use crate::session::{Session, PROTOCOL_VERSION};
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

/// Loopback-only QUIC game server (SPEC.md sections 124-146).
pub struct GameServer {
    pub endpoint: Endpoint,
    pub local_addr: SocketAddr,
    game: Arc<Mutex<GameInstance>>,
    /// Network metrics (SPEC.md section 190). POLICY domain: counters only,
    /// never gameplay. Optional so loopback tests can run without one.
    metrics: Option<Arc<Mutex<arpg_metrics::Metrics>>>,
    /// Empty-grace lifecycle guard (SPEC.md section 149): observes the
    /// player count and counts the grace down when the game is empty.
    guard: Arc<Mutex<crate::policy::GameGuard>>,
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
        let guard = Arc::new(Mutex::new(crate::policy::GameGuard::new(
            crate::policy::ServerPolicy::default(),
        )));
        Ok((
            GameServer {
                endpoint,
                local_addr,
                game,
                metrics: None,
                guard,
            },
            cert_der,
        ))
    }

    /// Access the empty-grace guard (section 149): the host loop ticks it
    /// and runs the save-then-destroy pipeline when it reports expiry.
    pub fn guard(&self) -> &Arc<Mutex<crate::policy::GameGuard>> {
        &self.guard
    }

    /// Attach a metrics collector for network byte counters (section 190).
    pub fn set_metrics(&mut self, metrics: Arc<Mutex<arpg_metrics::Metrics>>) {
        self.metrics = Some(metrics);
    }

    fn record_sent(&self, len: usize) {
        if let Some(m) = &self.metrics {
            m.lock()
                .unwrap()
                .add(arpg_metrics::names::NETWORK_BYTES_SENT, len as u64);
        }
    }

    fn record_received(&self, len: usize) {
        if let Some(m) = &self.metrics {
            m.lock()
                .unwrap()
                .add(arpg_metrics::names::NETWORK_BYTES_RECEIVED, len as u64);
        }
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
        self.record_received(hello_bytes.len());
        let hello = match decode_frame::<msg::ClientHello>(&hello_bytes) {
            Ok(h) => h,
            Err(_) => return,
        };
        let mut session = Session::new(arpg_core::PlayerId(0));
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
        let reply_bytes = encode_frame(&reply);
        self.record_sent(reply_bytes.len());
        if send.write_all(&reply_bytes).await.is_err() {
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
        self.record_received(join_bytes.len());
        let join = match decode_frame::<msg::JoinGameRequest>(&join_bytes) {
            Ok(j) => j,
            Err(_) => return,
        };
        let player = arpg_core::PlayerId(join.character_id);
        tracing::info!(player_identity_id = player.0, "join request");
        let (server_tick, input_delay, datapack_hash, tick_rate) = {
            let mut game = self.game.lock().unwrap();
            if game.rules.max_players as usize <= game.state.players.len() {
                // game full: reject
                return;
            }
            let pos = arpg_core::WorldPos::new(0, 0);
            game.add_player(player, pos);
            self.guard
                .lock()
                .unwrap()
                .observe_players(game.state.players.len());
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
        let accepted_bytes = encode_frame(&accepted_msg);
        self.record_sent(accepted_bytes.len());
        if send.write_all(&accepted_bytes).await.is_err() {
            return;
        }

        // --- Post-join: streams (reliable) + datagrams (unreliable, section 136) ---
        // Movement commands may arrive as datagrams; loss is acceptable since
        // a newer movement intent supersedes an older one. Everything else
        // (inventory, trade, skills with side effects) stays on the reliable
        // stream.
        let game = Arc::clone(&self.game);
        let conn_for_datagrams = connection.clone();
        let datagram_task = tokio::spawn(async move {
            loop {
                let buf = match conn_for_datagrams.read_datagram().await {
                    Ok(b) => b,
                    Err(_) => break,
                };
                if let Ok(wire) = decode_frame::<msg::CommandEnvelope>(&buf) {
                    // only movement is allowed on the unreliable path
                    if matches!(wire.command, Some(msg::command_envelope::Command::Move(_))) {
                        if let Some(sim_env) = crate::bridge::wire_to_sim(&wire) {
                            let mut g = game.lock().unwrap();
                            g.submit_command(sim_env);
                        }
                    }
                }
            }
        });

        // Reliable command loop
        while let Ok(frame) = read_frame(&mut recv).await {
            self.record_received(frame.len());
            if !session.is_command_accepted() {
                break;
            }
            let wire = match decode_frame::<msg::CommandEnvelope>(&frame) {
                Ok(w) => w,
                Err(_) => continue, // invalid input: ignored, never a panic (section 170)
            };
            tracing::trace!(
                player_identity_id = wire.player_id,
                command_sequence = wire.sequence,
                "command received on stream"
            );
            if let Some(sim_env) = crate::bridge::wire_to_sim(&wire) {
                let ack = {
                    let mut game = self.game.lock().unwrap();
                    game.submit_command(sim_env);
                    session.record_processed(&wire);
                    msg::CommandAck {
                        player_id: wire.player_id,
                        last_processed_sequence: wire.sequence,
                        execute_tick: game.state.tick.0,
                        admission: "Accepted".into(),
                    }
                };
                let ack_bytes = encode_frame(&ack);
                self.record_sent(ack_bytes.len());
                if send.write_all(&ack_bytes).await.is_err() {
                    break;
                }
            }
        }
        datagram_task.abort();
        session.disconnect();
        // sections 146-149: observe the player count after the session
        // ends; the empty-grace guard starts counting down when the game
        // becomes empty. The character save + GameState destroy pipeline
        // runs at guard expiry (section 149), driven by the host loop.
        let players_left = {
            let game = self.game.lock().unwrap();
            game.state.players.len().saturating_sub(1)
        };
        self.guard.lock().unwrap().observe_players(players_left);
    }
}
