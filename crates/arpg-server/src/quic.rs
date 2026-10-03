use crate::session::{Session, PROTOCOL_VERSION};
use arpg_core::PlayerId;
use arpg_protocol::messages as msg;
use arpg_sim::GameInstance;
use prost::Message;
use quinn::{Endpoint, ServerConfig};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::io::AsyncReadExt;
use tokio::sync::mpsc;

/// Message framing: 4-byte big-endian length prefix, then prost payload.
pub const MAX_FRAME_BYTES: usize = 1 << 20;

#[derive(Debug)]
pub enum FrameError {
    TooLarge,
    Decode,
    Io(std::io::Error),
}

pub fn encode_frame<M: Message>(msg: &M) -> Vec<u8> {
    let payload = msg.encode_to_vec();
    let mut out = Vec::with_capacity(4 + payload.len());
    out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    out.extend_from_slice(&payload);
    out
}

pub fn decode_frame<M: Message + Default>(buf: &[u8]) -> Result<M, FrameError> {
    if buf.len() > MAX_FRAME_BYTES {
        return Err(FrameError::TooLarge);
    }
    M::decode(buf).map_err(|_| FrameError::Decode)
}

pub async fn read_frame<S: AsyncReadExt + Unpin>(stream: &mut S) -> Result<Vec<u8>, FrameError> {
    let mut len_buf = [0u8; 4];
    stream
        .read_exact(&mut len_buf)
        .await
        .map_err(FrameError::Io)?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > MAX_FRAME_BYTES {
        return Err(FrameError::TooLarge);
    }
    let mut payload = vec![0u8; len];
    stream
        .read_exact(&mut payload)
        .await
        .map_err(FrameError::Io)?;
    Ok(payload)
}

/// Loopback-only QUIC game server (SPEC.md sections 124-134).
pub struct GameServer {
    pub endpoint: Endpoint,
    pub local_addr: SocketAddr,
    game: Arc<Mutex<GameInstance>>,
}

impl GameServer {
    /// Bind strictly to loopback. Never expose this server publicly.
    /// Returns the server together with its self-signed DER certificate so
    /// that local test clients can trust it.
    pub fn bind_loopback(
        game: Arc<Mutex<GameInstance>>,
    ) -> Result<(GameServer, Vec<u8>), quinn::ConnectionError> {
        let (server_config, cert_der) = Self::self_signed_config().expect("server config");
        let endpoint =
            Endpoint::server(server_config, "127.0.0.1:0".parse().unwrap()).expect("bind loopback");
        let local_addr = endpoint.local_addr().expect("local addr");
        Ok((
            GameServer {
                endpoint,
                local_addr,
                game,
            },
            cert_der,
        ))
    }

    fn self_signed_config() -> Result<(ServerConfig, Vec<u8>), Box<dyn std::error::Error>> {
        let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()])?;
        let key = rustls_pki_types::PrivateKeyDer::Pkcs8(cert.key_pair.serialize_der().into());
        let cert_der = rustls_pki_types::CertificateDer::from(cert.cert.der().to_vec());
        let der = cert_der.to_vec();
        Ok((ServerConfig::with_single_cert(vec![cert_der], key)?, der))
    }

    /// Serve connections until the endpoint is closed.
    pub async fn serve(self: Arc<Self>, mut shutdown: mpsc::Receiver<()>) {
        loop {
            tokio::select! {
                incoming = self.endpoint.accept() => {
                    match incoming {
                        Some(conn) => {
                            let server = Arc::clone(&self);
                            tokio::spawn(async move {
                                if let Ok(connection) = conn.await {
                                    server.handle_connection(connection).await;
                                }
                            });
                        }
                        None => break,
                    }
                }
                _ = shutdown.recv() => break,
            }
        }
    }

    async fn handle_connection(self: Arc<Self>, connection: quinn::Connection) {
        let (mut send, mut recv) = match connection.accept_bi().await {
            Ok(pair) => pair,
            Err(_) => return,
        };

        // --- Handshake (section 125) ---
        let hello_bytes = match read_frame(&mut recv).await {
            Ok(b) => b,
            Err(_) => return,
        };
        let hello = match decode_frame::<msg::ClientHello>(&hello_bytes) {
            Ok(h) => h,
            Err(_) => return,
        };
        let mut session = Session::new(PlayerId(0));
        let accepted = session.handle_client_hello(&hello);
        let reply = msg::ServerHello {
            protocol_version: PROTOCOL_VERSION.0,
            server_build: 1,
            accepted,
            reject_reason: if accepted {
                String::new()
            } else {
                "incompatible protocol".into()
            },
        };
        if send.write_all(&encode_frame(&reply)).await.is_err() {
            return;
        }
        if !accepted {
            send.finish().ok();
            // hold the stream open until the client closes it, so the
            // rejection reply is reliably delivered
            let mut sink = [0u8; 16];
            let _ = recv.read(&mut sink).await;
            return;
        }

        // --- JoinGame ---
        let join_bytes = match read_frame(&mut recv).await {
            Ok(b) => b,
            Err(_) => return,
        };
        let join = match decode_frame::<msg::JoinGameRequest>(&join_bytes) {
            Ok(j) => j,
            Err(_) => return,
        };
        let player = PlayerId(join.character_id);
        let (server_tick, input_delay, datapack_hash, tick_rate) = {
            let mut game = self.game.lock().unwrap();
            if game.rules.max_players as usize <= game.state.players.len() {
                // game full: reject
                return;
            }
            let pos = arpg_core::WorldPos::new(0, 0);
            game.add_player(player, pos);
            session.player = player;
            session.handle_join_accepted(game.state.tick);
            (
                game.state.tick.0,
                arpg_sim::DEFAULT_INPUT_DELAY_TICKS as u32,
                game.data.content_hash.to_vec(),
                arpg_core::TICKS_PER_SECOND,
            )
        };
        let accepted_msg = msg::JoinGameAccepted {
            game_id: join.game_id,
            player_id: player.0,
            player_slot: player.0,
            tick_rate,
            ruleset_hash: vec![],
            datapack_hash,
            server_tick,
            input_delay_ticks: input_delay,
        };
        if send.write_all(&encode_frame(&accepted_msg)).await.is_err() {
            return;
        }

        // --- Command loop: relay envelopes into the sim ---
        while let Ok(frame) = read_frame(&mut recv).await {
            if !session.is_command_accepted() {
                break;
            }
            let wire = match decode_frame::<msg::CommandEnvelope>(&frame) {
                Ok(w) => w,
                Err(_) => continue, // invalid input: ignored, never a panic (section 170)
            };
            if let Some(sim_env) = crate::bridge::wire_to_sim(&wire) {
                let ack = {
                    let mut game = self.game.lock().unwrap();
                    game.submit_command(sim_env.clone());
                    session.record_processed(&wire);
                    msg::CommandAck {
                        player_id: wire.player_id,
                        last_processed_sequence: wire.sequence,
                        execute_tick: game.state.tick.0,
                        admission: "Accepted".into(),
                    }
                };
                if send.write_all(&encode_frame(&ack)).await.is_err() {
                    break;
                }
            }
        }
        session.disconnect();
    }
}
