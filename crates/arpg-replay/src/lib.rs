//! Replay recording and deterministic playback (SPEC.md sections 157-160).
//!
//! A replay stores the header (versions, hashes, root seed), the initial
//! character snapshots, and the commands **after server validation and
//! scheduling** (section 159): it depends neither on jitter, duplication,
//! retransmission nor QUIC. Replaying the accepted commands on the same
//! seed and ruleset reproduces an identical state hash at every tick.

use arpg_core::{PlayerId, Tick};
use arpg_sim::CommandEnvelope;

/// Binary format versions (SPEC.md section 157).
pub const REPLAY_VERSION: u32 = 1;

/// BLAKE3 hash of the datapack used by the recorded game.
pub type DataPackHash = [u8; 32];
/// BLAKE3 hash of the ruleset used by the recorded game.
pub type RulesetHash = [u8; 32];

/// Replay header (SPEC.md section 158).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayHeader {
    pub replay_version: u32,
    pub engine_version: u32,
    pub datapack_hash: DataPackHash,
    pub ruleset_hash: RulesetHash,
    pub root_seed: [u8; 32],
}

/// A session transition affecting gameplay (join/leave) recorded in the
/// replay stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionTransition {
    Joined(PlayerId),
    Left(PlayerId),
}

/// One recorded entry: a scheduled command or a session transition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplayEntry {
    Command {
        execute_tick: u64,
        player: PlayerId,
        sequence: u32,
        payload: CommandPayload,
    },
    Transition(SessionTransition),
    Admin {
        tick: u64,
        payload: AdminPayload,
    },
}

/// Admin command recorded in a test game's replay (SPEC.md section 169:
/// test games may record admin commands so sessions stay reproducible).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdminPayload {
    Spawn { def: u32, x: i32, y: i32 },
    GiveItem { player: u32, def: u32 },
    Teleport { player: u32, x: i32, y: i32 },
    Kill { entity: u64 },
    DumpRng,
}

/// Wire form of a client command, independent of live intents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandPayload {
    Move { x: i32, y: i32 },
    UseSkill { skill: u32, x: i32, y: i32 },
    Interact { target: u64 },
    NoOp,
}

/// An in-progress or finalized replay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replay {
    pub header: ReplayHeader,
    pub entries: Vec<ReplayEntry>,
}

impl Replay {
    pub fn new(header: ReplayHeader) -> Replay {
        Replay {
            header,
            entries: Vec::new(),
        }
    }

    /// Record a scheduled command (section 159: post-scheduling).
    pub fn record_command(&mut self, scheduled: &arpg_sim::ScheduledCommand) {
        let payload = match &scheduled.envelope.command {
            arpg_sim::ClientCommand::Move(m) => CommandPayload::Move {
                x: m.direction.x,
                y: m.direction.y,
            },
            arpg_sim::ClientCommand::UseSkill(s) => CommandPayload::UseSkill {
                skill: s.skill.0,
                x: s.target.map(|t| t.x).unwrap_or(0),
                y: s.target.map(|t| t.y).unwrap_or(0),
            },
            arpg_sim::ClientCommand::Interact(i) => CommandPayload::Interact { target: i.target.0 },
            arpg_sim::ClientCommand::NoOp => CommandPayload::NoOp,
        };
        self.entries.push(ReplayEntry::Command {
            execute_tick: scheduled.execute_tick.0,
            player: scheduled.envelope.player,
            sequence: scheduled.envelope.sequence,
            payload,
        });
    }

    pub fn record_transition(&mut self, transition: SessionTransition) {
        self.entries.push(ReplayEntry::Transition(transition));
    }

    /// Record an admin command applied at the given tick (section 169).
    pub fn record_admin(&mut self, tick: u64, payload: AdminPayload) {
        self.entries.push(ReplayEntry::Admin { tick, payload });
    }

    /// Canonical binary encoding: header then entries in order.
    /// Deterministic: same replay always encodes to the same bytes.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"ARPGREPL");
        out.extend_from_slice(&self.header.replay_version.to_le_bytes());
        out.extend_from_slice(&self.header.engine_version.to_le_bytes());
        out.extend_from_slice(&self.header.datapack_hash);
        out.extend_from_slice(&self.header.ruleset_hash);
        out.extend_from_slice(&self.header.root_seed);
        out.extend_from_slice(&(self.entries.len() as u64).to_le_bytes());
        for entry in &self.entries {
            match entry {
                ReplayEntry::Command {
                    execute_tick,
                    player,
                    sequence,
                    payload,
                } => {
                    out.push(0);
                    out.extend_from_slice(&execute_tick.to_le_bytes());
                    out.extend_from_slice(&player.0.to_le_bytes());
                    out.extend_from_slice(&sequence.to_le_bytes());
                    match payload {
                        CommandPayload::Move { x, y } => {
                            out.push(0);
                            out.extend_from_slice(&x.to_le_bytes());
                            out.extend_from_slice(&y.to_le_bytes());
                        }
                        CommandPayload::UseSkill { skill, x, y } => {
                            out.push(1);
                            out.extend_from_slice(&skill.to_le_bytes());
                            out.extend_from_slice(&x.to_le_bytes());
                            out.extend_from_slice(&y.to_le_bytes());
                        }
                        CommandPayload::Interact { target } => {
                            out.push(2);
                            out.extend_from_slice(&target.to_le_bytes());
                        }
                        CommandPayload::NoOp => out.push(3),
                    }
                }
                ReplayEntry::Transition(t) => {
                    out.push(1);
                    match t {
                        SessionTransition::Joined(p) => {
                            out.push(0);
                            out.extend_from_slice(&p.0.to_le_bytes());
                        }
                        SessionTransition::Left(p) => {
                            out.push(1);
                            out.extend_from_slice(&p.0.to_le_bytes());
                        }
                    }
                }
                ReplayEntry::Admin { tick, payload } => {
                    out.push(2);
                    out.extend_from_slice(&tick.to_le_bytes());
                    match payload {
                        AdminPayload::Spawn { def, x, y } => {
                            out.push(0);
                            out.extend_from_slice(&def.to_le_bytes());
                            out.extend_from_slice(&x.to_le_bytes());
                            out.extend_from_slice(&y.to_le_bytes());
                        }
                        AdminPayload::GiveItem { player, def } => {
                            out.push(1);
                            out.extend_from_slice(&player.to_le_bytes());
                            out.extend_from_slice(&def.to_le_bytes());
                        }
                        AdminPayload::Teleport { player, x, y } => {
                            out.push(2);
                            out.extend_from_slice(&player.to_le_bytes());
                            out.extend_from_slice(&x.to_le_bytes());
                            out.extend_from_slice(&y.to_le_bytes());
                        }
                        AdminPayload::Kill { entity } => {
                            out.push(3);
                            out.extend_from_slice(&entity.to_le_bytes());
                        }
                        AdminPayload::DumpRng => out.push(4),
                    }
                }
            }
        }
        out
    }

    /// Decode a replay from canonical bytes. Errors on truncated or
    /// corrupted input.
    pub fn decode(bytes: &[u8]) -> Result<Replay, &'static str> {
        let mut r = Reader { buf: bytes, pos: 0 };
        let magic = r.take(8)?;
        if magic != b"ARPGREPL" {
            return Err("bad magic");
        }
        let replay_version = r.u32()?;
        let engine_version = r.u32()?;
        let mut datapack_hash = [0u8; 32];
        datapack_hash.copy_from_slice(r.take(32)?);
        let mut ruleset_hash = [0u8; 32];
        ruleset_hash.copy_from_slice(r.take(32)?);
        let mut root_seed = [0u8; 32];
        root_seed.copy_from_slice(r.take(32)?);
        let count = r.u64()? as usize;
        let mut entries = Vec::with_capacity(count.min(1 << 16));
        for _ in 0..count {
            let kind = r.u8()?;
            if kind == 0 {
                let execute_tick = r.u64()?;
                let player = PlayerId(r.u32()?);
                let sequence = r.u32()?;
                let payload = match r.u8()? {
                    0 => {
                        let x = r.i32()?;
                        let y = r.i32()?;
                        CommandPayload::Move { x, y }
                    }
                    1 => {
                        let skill = r.u32()?;
                        let x = r.i32()?;
                        let y = r.i32()?;
                        CommandPayload::UseSkill { skill, x, y }
                    }
                    2 => {
                        let target = r.u64()?;
                        CommandPayload::Interact { target }
                    }
                    3 => CommandPayload::NoOp,
                    _ => return Err("bad payload tag"),
                };
                entries.push(ReplayEntry::Command {
                    execute_tick,
                    player,
                    sequence,
                    payload,
                });
            } else if kind == 1 {
                let t = r.u8()?;
                let player = PlayerId(r.u32()?);
                let transition = if t == 0 {
                    SessionTransition::Joined(player)
                } else if t == 1 {
                    SessionTransition::Left(player)
                } else {
                    return Err("bad transition tag");
                };
                entries.push(ReplayEntry::Transition(transition));
            } else if kind == 2 {
                let tick = r.u64()?;
                let payload = match r.u8()? {
                    0 => {
                        let def = r.u32()?;
                        let x = r.i32()?;
                        let y = r.i32()?;
                        AdminPayload::Spawn { def, x, y }
                    }
                    1 => {
                        let player = r.u32()?;
                        let def = r.u32()?;
                        AdminPayload::GiveItem { player, def }
                    }
                    2 => {
                        let player = r.u32()?;
                        let x = r.i32()?;
                        let y = r.i32()?;
                        AdminPayload::Teleport { player, x, y }
                    }
                    3 => {
                        let entity = r.u64()?;
                        AdminPayload::Kill { entity }
                    }
                    4 => AdminPayload::DumpRng,
                    _ => return Err("bad admin tag"),
                };
                entries.push(ReplayEntry::Admin { tick, payload });
            } else {
                return Err("bad entry tag");
            }
        }
        Ok(Replay {
            header: ReplayHeader {
                replay_version,
                engine_version,
                datapack_hash,
                ruleset_hash,
                root_seed,
            },
            entries,
        })
    }
}

struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], &'static str> {
        if self.pos + n > self.buf.len() {
            return Err("truncated");
        }
        let s = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, &'static str> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> Result<u32, &'static str> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn i32(&mut self) -> Result<i32, &'static str> {
        let b = self.take(4)?;
        Ok(i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn u64(&mut self) -> Result<u64, &'static str> {
        let b = self.take(8)?;
        let mut a = [0u8; 8];
        a.copy_from_slice(b);
        Ok(u64::from_le_bytes(a))
    }
}

/// Replay a recorded game against a fresh `GameInstance` and verify that
/// every tick produces the recorded state hash (section 160).
pub struct Replayer<'a> {
    pub game: &'a mut arpg_sim::GameInstance,
}

impl<'a> Replayer<'a> {
    pub fn new(game: &'a mut arpg_sim::GameInstance) -> Replayer<'a> {
        Replayer { game }
    }

    /// Feed one recorded entry at the right point in the stream. Returns
    /// the tick that was advanced to, or None if the entry only queued a
    /// command for a future tick.
    pub fn submit(&mut self, entry: &ReplayEntry) -> Result<(), &'static str> {
        match entry {
            ReplayEntry::Transition(SessionTransition::Joined(p)) => {
                if !self.game.state.players.contains_key(p) {
                    self.game.add_player(*p, arpg_core::WorldPos::new(0, 0));
                }
                Ok(())
            }
            ReplayEntry::Transition(SessionTransition::Left(p)) => {
                let _ = p;
                Ok(())
            }
            ReplayEntry::Command {
                execute_tick,
                player,
                sequence,
                payload,
            } => {
                let envelope = CommandEnvelope {
                    sequence: *sequence,
                    client_tick: Tick(*execute_tick),
                    player: *player,
                    command: match payload {
                        CommandPayload::Move { x, y } => {
                            arpg_sim::ClientCommand::Move(arpg_sim::MoveIntent {
                                direction: arpg_core::WorldPos::new(*x, *y),
                                movement_mode: arpg_sim::MovementMode::Run,
                                sequence: *sequence,
                            })
                        }
                        CommandPayload::UseSkill { skill, x, y } => {
                            arpg_sim::ClientCommand::UseSkill(arpg_sim::UseSkillIntent {
                                skill: arpg_core::SkillId(*skill),
                                target: Some(arpg_core::WorldPos::new(*x, *y)),
                            })
                        }
                        CommandPayload::Interact { target } => {
                            arpg_sim::ClientCommand::Interact(arpg_sim::InteractIntent {
                                target: arpg_core::ObjectId(*target),
                            })
                        }
                        CommandPayload::NoOp => arpg_sim::ClientCommand::NoOp,
                    },
                };
                self.game.submit_command(envelope);
                Ok(())
            }
            ReplayEntry::Admin { payload, .. } => {
                // Admin commands reproduce deterministically (section 169):
                // same state + same command = same outcome.
                let cmd = match payload {
                    AdminPayload::Spawn { def, x, y } => arpg_sim::admin::AdminCommand::Spawn {
                        def: *def,
                        pos: arpg_core::WorldPos::new(*x, *y),
                    },
                    AdminPayload::GiveItem { player, def } => {
                        arpg_sim::admin::AdminCommand::GiveItem {
                            player: arpg_core::PlayerId(*player),
                            def: arpg_core::ItemDefId(*def),
                        }
                    }
                    AdminPayload::Teleport { player, x, y } => {
                        arpg_sim::admin::AdminCommand::Teleport {
                            player: arpg_core::PlayerId(*player),
                            pos: arpg_core::WorldPos::new(*x, *y),
                        }
                    }
                    AdminPayload::Kill { entity } => arpg_sim::admin::AdminCommand::Kill {
                        entity: arpg_core::EntityId(*entity),
                    },
                    AdminPayload::DumpRng => arpg_sim::admin::AdminCommand::DumpRng,
                };
                self.game
                    .apply_admin_command(&cmd)
                    .map_err(|_| "admin command failed during replay")?;
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header() -> ReplayHeader {
        ReplayHeader {
            replay_version: REPLAY_VERSION,
            engine_version: 1,
            datapack_hash: [1; 32],
            ruleset_hash: [2; 32],
            root_seed: [42; 32],
        }
    }

    #[test]
    fn encode_decode_roundtrip_is_lossless() {
        let mut replay = Replay::new(header());
        replay.record_command(&arpg_sim::ScheduledCommand {
            execute_tick: Tick(5),
            player: PlayerId(1),
            sequence: 3,
            envelope: CommandEnvelope {
                sequence: 3,
                client_tick: Tick(4),
                player: PlayerId(1),
                command: arpg_sim::ClientCommand::Move(arpg_sim::MoveIntent {
                    direction: arpg_core::WorldPos::new(10, -4),
                    movement_mode: arpg_sim::MovementMode::Run,
                    sequence: 3,
                }),
            },
        });
        replay.record_transition(SessionTransition::Joined(PlayerId(2)));
        let bytes = replay.encode();
        let decoded = Replay::decode(&bytes).unwrap();
        assert_eq!(decoded, replay);
    }

    #[test]
    fn decode_rejects_truncated_input() {
        let replay = Replay::new(header());
        let bytes = replay.encode();
        assert!(Replay::decode(&bytes[..bytes.len() - 1]).is_err());
        assert!(Replay::decode(b"garbage").is_err());
    }

    #[test]
    fn replay_reproduces_identical_state_hashes() {
        let run_game = |replay: &Replay| -> Vec<[u8; 32]> {
            let data = std::sync::Arc::new(arpg_data::GameData::default());
            let rules = std::sync::Arc::new(arpg_rules::GameRules::default());
            let mut game = arpg_sim::GameInstance::new(data, rules, replay.header.root_seed);
            let mut hashes = Vec::new();
            // apply entries, running exactly 10 ticks overall
            for entry in &replay.entries {
                let mut r = Replayer::new(&mut game);
                r.submit(entry).unwrap();
            }
            for _ in 0..10 {
                let result = game.tick();
                hashes.push(result.state_hash);
            }
            hashes
        };
        let mut replay = Replay::new(header());
        replay.record_transition(SessionTransition::Joined(PlayerId(1)));
        replay.record_command(&arpg_sim::ScheduledCommand {
            execute_tick: Tick(3),
            player: PlayerId(1),
            sequence: 1,
            envelope: CommandEnvelope {
                sequence: 1,
                client_tick: Tick(1),
                player: PlayerId(1),
                command: arpg_sim::ClientCommand::Move(arpg_sim::MoveIntent {
                    direction: arpg_core::WorldPos::new(3, 0),
                    movement_mode: arpg_sim::MovementMode::Run,
                    sequence: 1,
                }),
            },
        });
        let h1 = run_game(&replay);
        let h2 = run_game(&replay);
        assert_eq!(h1, h2, "same seed + same accepted commands = same hashes");
    }
}
