use crate::command::{ClientCommand, CommandEnvelope};
use crate::phase::Phase;
use crate::scheduler::{ScheduledCommand, Scheduler};
use arpg_core::{PlayerId, Tick, WorldPos};
use std::collections::BTreeMap;
use std::sync::Arc;

pub type RootSeed = [u8; 32];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameConfig {
    pub seed: RootSeed,
    pub difficulty: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerState {
    pub player: PlayerId,
    pub pos: WorldPos,
    pub life: i64,
    pub mana: i64,
}

#[derive(Debug, Default)]
pub struct GameState {
    pub tick: Tick,
    pub players: BTreeMap<PlayerId, PlayerState>,
    pub attack_sequence: u64,
    pub ai_decision_sequence: u64,
    pub spawn_sequence: u64,
    pub death_sequence: u64,
    pub merchant_refresh_sequence: u64,
}

impl GameState {
    pub fn canonical_hash_input(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&self.tick.0.to_le_bytes());
        for (id, p) in &self.players {
            buf.extend_from_slice(&id.0.to_le_bytes());
            buf.extend_from_slice(&p.pos.x.to_le_bytes());
            buf.extend_from_slice(&p.pos.y.to_le_bytes());
            buf.extend_from_slice(&p.life.to_le_bytes());
            buf.extend_from_slice(&p.mana.to_le_bytes());
        }
        for seq in [
            self.attack_sequence,
            self.ai_decision_sequence,
            self.spawn_sequence,
            self.death_sequence,
            self.merchant_refresh_sequence,
        ] {
            buf.extend_from_slice(&seq.to_le_bytes());
        }
        buf
    }

    pub fn state_hash(&self) -> [u8; 32] {
        arpg_core::hash::state_hash(&self.canonical_hash_input())
    }
}

pub struct GameInstance {
    pub data: Arc<arpg_data::GameData>,
    pub rules: Arc<arpg_rules::GameRules>,
    pub state: GameState,
    scheduler: Scheduler,
    event_log: Vec<arpg_core::GameEvent>,
}

pub struct TickResult {
    pub tick: Tick,
    pub state_hash: [u8; 32],
    pub events: Vec<arpg_core::GameEvent>,
}

impl GameInstance {
    pub fn new(
        data: Arc<arpg_data::GameData>,
        rules: Arc<arpg_rules::GameRules>,
        seed: RootSeed,
    ) -> GameInstance {
        let _ = seed;
        GameInstance {
            data,
            rules,
            state: GameState::default(),
            scheduler: Scheduler::new(),
            event_log: Vec::new(),
        }
    }

    pub fn add_player(&mut self, player: PlayerId, pos: WorldPos) {
        self.state.players.insert(
            player,
            PlayerState {
                player,
                pos,
                life: 100,
                mana: 50,
            },
        );
    }

    pub fn submit_command(&mut self, envelope: CommandEnvelope, execute_tick: Tick) -> bool {
        let scheduled = ScheduledCommand {
            execute_tick,
            player: envelope.player,
            sequence: envelope.sequence,
            envelope,
        };
        self.scheduler.schedule(scheduled)
    }

    pub fn tick(&mut self) -> TickResult {
        self.state.tick = self.state.tick.next();
        let tick = self.state.tick;
        let due = self.scheduler.take_due(tick);

        for phase in crate::phase::PHASES.iter() {
            self.run_phase(*phase, tick, &due);
        }

        let events = std::mem::take(&mut self.event_log);
        TickResult {
            tick,
            state_hash: self.state.state_hash(),
            events,
        }
    }

    fn run_phase(&mut self, phase: Phase, tick: Tick, due: &[ScheduledCommand]) {
        match phase {
            Phase::UpdatePlayerIntent => {
                for cmd in due {
                    if let ClientCommand::Move(intent) = &cmd.envelope.command {
                        if let Some(p) = self.state.players.get_mut(&cmd.player) {
                            p.pos = intent.direction;
                        }
                    }
                }
            }
            Phase::BeginTick | Phase::EndTick => {
                let _ = tick;
            }
            _ => {}
        }
    }
}
