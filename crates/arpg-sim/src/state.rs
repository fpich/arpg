use crate::command::{Admission, ClientCommand, CommandEnvelope};
use crate::phase::Phase;
use crate::replication::ReplicationTracker;
use crate::scheduler::{CommandQueue, ScheduledCommand, Scheduler, DEFAULT_INPUT_DELAY_TICKS};
use arpg_core::{EntityId, EventBuffer, EventOrderKey, GameEvent, PlayerId, Tick, WorldPos};
use arpg_world::LevelInstance;
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
    /// Desired movement targets recorded during UpdatePlayerIntent and
    /// consumed during MovementResolution (SPEC.md section 25).
    pub movement_intents: BTreeMap<PlayerId, WorldPos>,
    pub entities_alive: u64,
    pub attack_sequence: u64,
    pub ai_decision_sequence: u64,
    pub spawn_sequence: u64,
    pub death_sequence: u64,
    pub merchant_refresh_sequence: u64,
    pub event_sequence: u64,
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
        buf.extend_from_slice(&self.entities_alive.to_le_bytes());
        for seq in [
            self.attack_sequence,
            self.ai_decision_sequence,
            self.spawn_sequence,
            self.death_sequence,
            self.merchant_refresh_sequence,
            self.event_sequence,
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
    /// Active level; when None, movement applies without terrain collision
    /// (used by tests and headless instances without a generated world).
    pub level: Option<LevelInstance>,
    scheduler: Scheduler,
    command_queue: CommandQueue,
    event_buffer: EventBuffer,
    pub replication: ReplicationTracker,
}

pub struct TickResult {
    pub tick: Tick,
    pub state_hash: [u8; 32],
    pub events: Vec<GameEvent>,
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
            level: None,
            scheduler: Scheduler::new(),
            command_queue: CommandQueue::new(),
            event_buffer: EventBuffer::new(),
            replication: ReplicationTracker::new(),
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
        let key = self.next_event_key(EntityId(player.0 as u64));
        self.event_buffer.emit(key, GameEvent::PlayerJoined(player));
    }

    fn next_event_key(&mut self, source: EntityId) -> EventOrderKey {
        self.state.event_sequence += 1;
        EventOrderKey {
            priority: 0,
            target: source,
            source,
            sequence: self.state.event_sequence,
        }
    }

    /// Raw arrival: the envelope is queued and admitted during the next tick
    /// phases (IngestCommands/CanonicalizeCommands/ValidateCommands).
    pub fn submit_command(&mut self, envelope: CommandEnvelope) {
        self.command_queue.push(envelope);
    }

    pub fn tick(&mut self) -> TickResult {
        self.state.tick = self.state.tick.next();
        let tick = self.state.tick;

        let ingested = self.command_queue.drain();
        let mut scheduled: Vec<ScheduledCommand> = Vec::new();
        for envelope in ingested {
            let admission = self
                .scheduler
                .admit(envelope, tick, DEFAULT_INPUT_DELAY_TICKS);
            if let Admission::Accepted = admission {
                // kept in scheduler; collected via take_due below
            }
        }
        let due = self.scheduler.take_due(tick);

        for phase in crate::phase::PHASES.iter() {
            self.run_phase(*phase, tick, &due, &scheduled);
        }
        scheduled.clear();

        let events = self.event_buffer.drain_canonical();
        let mut hash_input = self.state.canonical_hash_input();
        hash_input.extend_from_slice(&self.scheduler.canonical_hash_input());
        let state_hash = arpg_core::hash::state_hash(&hash_input);

        TickResult {
            tick,
            state_hash,
            events,
        }
    }

    fn run_phase(
        &mut self,
        phase: Phase,
        _tick: Tick,
        due: &[ScheduledCommand],
        _scheduled: &[ScheduledCommand],
    ) {
        if let Phase::UpdatePlayerIntent = phase {
            for cmd in due {
                if let ClientCommand::Move(intent) = &cmd.envelope.command {
                    if self.state.players.contains_key(&cmd.player) {
                        self.state
                            .movement_intents
                            .insert(cmd.player, intent.direction);
                        let key = self.next_event_key(arpg_core::EntityId(cmd.player.0 as u64));
                        self.event_buffer.emit(
                            key,
                            GameEvent::EntitySpawned(arpg_core::EntityId(cmd.player.0 as u64)),
                        );
                    }
                }
            }
        }

        if let Phase::MovementResolution = phase {
            self.resolve_movement();
            self.state.movement_intents.clear();
        }
    }

    /// Movement resolution pipeline (SPEC.md sections 25-26): desired target,
    /// terrain collision against the level, then entity collision. Movers are
    /// processed in canonical EntityId order; the first mover into a tile wins
    /// and simultaneous contenders are blocked.
    fn resolve_movement(&mut self) {
        if self.state.movement_intents.is_empty() {
            return;
        }
        let mut moved: Vec<PlayerId> = Vec::new();
        for (&player, &target) in self.state.movement_intents.iter() {
            let Some(p) = self.state.players.get(&player) else {
                continue;
            };
            let from = p.pos;
            if from == target {
                continue;
            }
            if let Some(level) = &self.level {
                if !level.walkable_at(target) {
                    continue;
                }
            }
            let target_tile = target.tile();
            let occupied = self
                .state
                .players
                .values()
                .any(|other| other.player != player && other.pos.tile() == target_tile);
            if occupied {
                continue;
            }
            let Some(p) = self.state.players.get_mut(&player) else {
                continue;
            };
            p.pos = target;
            moved.push(player);
        }
        for player in moved {
            let _ = self.replication.touch(player);
        }
    }
}
