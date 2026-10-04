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
    /// Character snapshots produced at empty-grace expiry (sections
    /// 119-121, 149): save-then-destroy. Optional so loopback tests can
    /// run without persistence.
    repository: Option<Arc<Mutex<arpg_persistence::CharacterRepository>>>,
    /// Command rate limiter (SPEC.md section 171): admission control on
    /// the QUIC paths. Runs on server tick time, never modifies an
    /// already-accepted command.
    limiter: Arc<Mutex<crate::ratelimit::RateLimiter>>,
    /// Live snapshot subscribers (SPEC.md sections 143, 141): one
    /// bounded channel per connected player; the host loop pushes
    /// encoded wire snapshots each tick.
    subscribers: Arc<Mutex<std::collections::BTreeMap<arpg_core::PlayerId, mpsc::Sender<Vec<u8>>>>>,
}

impl GameServer {
    /// Bind strictly to loopback. Never expose this server publicly.
    /// Returns the server together with its self-signed DER certificate so
    /// that local test clients can trust it.
    pub fn bind_loopback(
        game: Arc<Mutex<GameInstance>>,
    ) -> Result<(GameServer, Vec<u8>), quinn::ConnectionError> {
        Self::bind_loopback_with_policy(game, crate::policy::ServerPolicy::default())
    }

    /// Bind strictly to loopback with an explicit lifecycle policy.
    pub fn bind_loopback_with_policy(
        game: Arc<Mutex<GameInstance>>,
        policy: crate::policy::ServerPolicy,
    ) -> Result<(GameServer, Vec<u8>), quinn::ConnectionError> {
        let (server_config, cert_der) = Self::self_signed_config().expect("server config");
        let endpoint =
            Endpoint::server(server_config, "127.0.0.1:0".parse().unwrap()).expect("bind loopback");
        let local_addr = endpoint.local_addr().expect("local addr");
        let guard = Arc::new(Mutex::new(crate::policy::GameGuard::new(policy)));
        let limiter = Arc::new(Mutex::new(crate::ratelimit::RateLimiter::new(
            crate::ratelimit::RatePolicy::reference(),
            8,
        )));
        Ok((
            GameServer {
                endpoint,
                local_addr,
                game,
                metrics: None,
                guard,
                repository: None,
                limiter,
                subscribers: Arc::new(Mutex::new(std::collections::BTreeMap::new())),
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

    /// Attach the character repository used at empty-grace expiry
    /// (sections 119-121, 149).
    pub fn set_repository(
        &mut self,
        repository: Arc<Mutex<arpg_persistence::CharacterRepository>>,
    ) {
        self.repository = Some(repository);
    }

    /// Background host loop (sections 148-149): ticks the game at the
    /// canonical tick rate, then the empty-grace guard; when the guard
    /// reports expiry the remaining characters are snapshotted and saved
    /// and the GameState is destroyed. Grace ticks only advance when the
    /// game is empty, so an active game ticks forever.
    fn host_loop(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let tick_dur = std::time::Duration::from_millis(arpg_core::TICK_DURATION_MS.into());
            let mut interval = tokio::time::interval(tick_dur);
            loop {
                interval.tick().await;
                let expired = {
                    let mut game = self.game.lock().unwrap();
                    game.tick();
                    self.limiter.lock().unwrap().tick();
                    self.guard.lock().unwrap().tick_empty()
                };
                {
                    let subscribers = {
                        let mut subs = self.subscribers.lock().unwrap();
                        subs.retain(|_, tx| !tx.is_closed());
                        subs.keys().copied().collect::<Vec<_>>()
                    };
                    for player in subscribers {
                        let (snapshot_bytes, last_seq) = {
                            let mut game = self.game.lock().unwrap();
                            let repl = game.replication_for(player);
                            let last_seq = game.scheduler_last_accepted(player).unwrap_or(0);
                            (
                                crate::bridge::replication_to_wire(&repl, &mut |_, _| {}),
                                last_seq,
                            )
                        };
                        let mut wire = snapshot_bytes;
                        wire.last_processed_command_sequence = last_seq;
                        let bytes = encode_frame(&wire);
                        self.record_sent(bytes.len());
                        let subs = self.subscribers.lock().unwrap();
                        if let Some(tx) = subs.get(&player) {
                            let _ = tx.try_send(bytes);
                        }
                    }
                }
                if expired {
                    let (players, snapshots) = {
                        let game = self.game.lock().unwrap();
                        let players: Vec<arpg_core::PlayerId> =
                            game.state.players.keys().copied().collect();
                        let snapshots = players
                            .iter()
                            .map(|p| (*p, game.character_snapshot(*p)))
                            .collect::<Vec<_>>();
                        (players, snapshots)
                    };
                    if let Some(repo) = &self.repository {
                        let mut repo = repo.lock().unwrap();
                        for (player, snapshot) in snapshots {
                            repo.save(player, snapshot);
                        }
                    }
                    tracing::info!(
                        players = players.len(),
                        "empty grace expired: characters saved, game destroyed"
                    );
                    {
                        let mut game = self.game.lock().unwrap();
                        game.destroy_state();
                    }
                }
            }
        })
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

    /// Serve connections until the endpoint is closed. The host loop
    /// starts with it and is aborted on shutdown.
    pub async fn serve(self: Arc<Self>, mut shutdown: mpsc::Receiver<()>) {
        let host_loop = Arc::clone(&self).host_loop();
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
        host_loop.abort();
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
        // rate-limit registration (section 171): capacity-bounded
        self.limiter.lock().unwrap().register(player);
        // snapshot subscription (sections 143, 141): the host loop pushes
        // encoded wire snapshots on this bounded channel each tick
        let (snap_tx, mut snap_rx) = mpsc::channel::<Vec<u8>>(64);
        self.subscribers.lock().unwrap().insert(player, snap_tx);
        let game = Arc::clone(&self.game);
        let limiter = Arc::clone(&self.limiter);
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
                        // rate-limit admission (section 171): a denied flood
                        // never reaches the scheduler and never earns budget
                        let decision = limiter
                            .lock()
                            .unwrap()
                            .check(player, crate::ratelimit::ArrivalKind::Command);
                        if decision == crate::ratelimit::RateDecision::Denied {
                            continue;
                        }
                        if let Some(sim_env) = crate::bridge::wire_to_sim(&wire) {
                            let mut g = game.lock().unwrap();
                            g.submit_command(sim_env);
                        }
                    }
                }
            }
        });

        // Reliable loop: multiplexes client commands and host-loop
        // snapshots on the same stream (sections 129, 143)
        loop {
            tokio::select! {
                frame = read_frame(&mut recv) => {
                    let Ok(frame) = frame else { break };
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
                // rate-limit admission (section 171): the kind is derived
                // from the payload, never trusted from the client
                let kind = crate::ratelimit::ArrivalKind::of_command(&sim_env);
                let decision = self.limiter.lock().unwrap().check(sim_env.player, kind);
                if decision == crate::ratelimit::RateDecision::Denied {
                    let ack = msg::CommandAck {
                        player_id: wire.player_id,
                        last_processed_sequence: wire.sequence,
                        execute_tick: 0,
                        admission: "RateLimited".into(),
                    };
                    let ack_bytes = encode_frame(&ack);
                    self.record_sent(ack_bytes.len());
                    if send.write_all(&ack_bytes).await.is_err() {
                        break;
                    }
                    continue;
                }
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
                Some(bytes) = snap_rx.recv() => {
                    // forward a host-loop snapshot (section 143)
                    self.record_sent(bytes.len());
                    if send.write_all(&bytes).await.is_err() {
                        break;
                    }
                }
            }
        }
        datagram_task.abort();
        self.subscribers.lock().unwrap().remove(&player);
        self.limiter.lock().unwrap().unregister(player);
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
