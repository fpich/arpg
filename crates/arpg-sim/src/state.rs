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
    pub interact_intents: BTreeMap<PlayerId, arpg_core::ObjectId>,
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
    pub actors: BTreeMap<EntityId, crate::actor::Actor>,
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
            actors: BTreeMap::new(),
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
        self.actors.insert(
            EntityId(player.0 as u64),
            crate::actor::Actor::new(EntityId(player.0 as u64)),
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
        if let Some(level) = &self.level {
            for obj in &level.objects {
                hash_input.extend_from_slice(&obj.id.0.to_le_bytes());
                hash_input.extend_from_slice(&(obj.state as u8).to_le_bytes());
                hash_input.extend_from_slice(&obj.cooldown_until.0.to_le_bytes());
            }
        }
        for (id, actor) in &self.actors {
            hash_input.extend_from_slice(&id.0.to_le_bytes());
            hash_input.extend_from_slice(&(actor.mode as u8).to_le_bytes());
            hash_input.extend_from_slice(&(actor.lifecycle as u8).to_le_bytes());
            if let Some(action) = &actor.action {
                hash_input.extend_from_slice(&action.id.to_le_bytes());
                hash_input.extend_from_slice(&(action.phase as u8).to_le_bytes());
            }
        }
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
                match &cmd.envelope.command {
                    ClientCommand::Move(intent) if self.state.players.contains_key(&cmd.player) => {
                        self.state
                            .movement_intents
                            .insert(cmd.player, intent.direction);
                        let key = self.next_event_key(arpg_core::EntityId(cmd.player.0 as u64));
                        self.event_buffer.emit(
                            key,
                            GameEvent::EntitySpawned(arpg_core::EntityId(cmd.player.0 as u64)),
                        );
                    }
                    ClientCommand::Interact(intent)
                        if self.state.players.contains_key(&cmd.player) =>
                    {
                        self.state
                            .interact_intents
                            .insert(cmd.player, intent.target);
                    }
                    _ => {}
                }
            }
        }

        if let Phase::ActionStateAdvance = phase {
            self.advance_actions(_tick);
        }

        if let Phase::PendingDeathResolution = phase {
            self.resolve_pending_deaths();
        }

        if let Phase::MovementResolution = phase {
            self.resolve_movement();
            self.state.movement_intents.clear();
        }

        if let Phase::InteractionResolution = phase {
            self.resolve_interactions(_tick);
            self.state.interact_intents.clear();
        }

        if let Phase::WorldObjectUpdate = phase {
            if let Some(level) = self.level.as_mut() {
                for obj in level.objects.iter_mut() {
                    if obj.state == arpg_core::ObjectInstanceState::OnRecharge
                        && self.state.tick >= obj.cooldown_until
                    {
                        obj.state = arpg_core::ObjectInstanceState::Default;
                    }
                }
            }
        }
    }

    /// Advance every in-flight action by one phase step (SPEC.md sections
    /// 32-33). Completed actions return the actor to Neutral unless an
    /// interruption was scheduled.
    fn advance_actions(&mut self, tick: arpg_core::Tick) {
        let mut completed: Vec<EntityId> = Vec::new();
        let mut interrupted: Vec<(EntityId, arpg_core::ActorMode)> = Vec::new();
        let mut resolved: Vec<EntityId> = Vec::new();
        for (id, actor) in self.actors.iter_mut() {
            if let Some(action) = &mut actor.action {
                if action.phase == arpg_core::ActionPhase::Impact {
                    resolved.push(*id);
                }
                if action.advance(tick) {
                    completed.push(*id);
                } else if action.pending_interrupt.is_some() {
                    if let Some(mode) = action.apply_interrupt() {
                        interrupted.push((*id, mode));
                    }
                }
            }
        }
        for id in resolved {
            let key = self.next_event_key(id);
            self.event_buffer.emit(key, GameEvent::ActionResolved(id));
        }
        for id in completed {
            if let Some(actor) = self.actors.get_mut(&id) {
                actor.action = None;
                if actor.mode != arpg_core::ActorMode::Dead {
                    actor.mode = arpg_core::ActorMode::Neutral;
                }
            }
        }
        for (id, mode) in interrupted {
            if let Some(actor) = self.actors.get_mut(&id) {
                actor.mode = mode;
            }
        }
    }

    /// PendingDeath -> Dead (SPEC.md section 13) with kill credit (section
    /// 14): the killer is the source of the first ordered event that brought
    /// life to <= 0.
    fn resolve_pending_deaths(&mut self) {
        let dead: Vec<EntityId> = self
            .actors
            .iter()
            .filter(|(_, a)| a.lifecycle == arpg_core::Lifecycle::PendingDeath)
            .map(|(id, _)| *id)
            .collect();
        for id in dead {
            if let Some(actor) = self.actors.get_mut(&id) {
                actor.lifecycle = arpg_core::Lifecycle::Dead;
                actor.mode = arpg_core::ActorMode::Dead;
                actor.action = None;
            }
            let key = self.next_event_key(id);
            self.event_buffer.emit(key, GameEvent::EntityRemoved(id));
        }
    }

    /// Mark an entity as PendingDeath (SPEC.md section 13) with kill credit.
    pub fn apply_damage(&mut self, target: EntityId, source: EntityId, amount: i64) {
        let Some(player_id) = self
            .state
            .players
            .keys()
            .find(|p| EntityId(p.0 as u64) == target)
            .copied()
        else {
            return;
        };
        let Some(p) = self.state.players.get_mut(&player_id) else {
            return;
        };
        let was_alive = p.life > 0;
        p.life = p.life.saturating_sub(amount);
        let now_dead = p.life <= 0;
        if let Some(actor) = self.actors.get_mut(&target) {
            if now_dead && actor.lifecycle == arpg_core::Lifecycle::Alive {
                actor.lifecycle = arpg_core::Lifecycle::PendingDeath;
            }
        }
        let key = self.next_event_key(source);
        self.event_buffer.emit(
            key,
            GameEvent::DamageApplied {
                target,
                source,
                amount,
            },
        );
        if was_alive && now_dead {
            let key = self.next_event_key(source);
            self.event_buffer.emit(
                key,
                GameEvent::EntityKilled {
                    target,
                    killer: source,
                },
            );
        }
    }

    /// Interaction resolution (SPEC.md section 98): each intent is validated
    /// against the target's distance, state and cooldown, then applied in
    /// canonical player order.
    fn resolve_interactions(&mut self, tick: arpg_core::Tick) {
        if self.state.interact_intents.is_empty() {
            return;
        }
        let intents: Vec<(PlayerId, arpg_core::ObjectId)> = self
            .state
            .interact_intents
            .iter()
            .map(|(p, o)| (*p, *o))
            .collect();
        for (player_id, target_id) in intents {
            let Some(actor) = self.state.players.get(&player_id) else {
                continue;
            };
            let actor_pos = actor.pos;
            let Some(level) = self.level.as_mut() else {
                continue;
            };
            let Some(obj) = level.objects.iter_mut().find(|o| o.id == target_id) else {
                continue;
            };
            if !obj.can_interact(actor_pos, tick) {
                continue;
            }
            use arpg_core::ObjectInstanceState as St;
            let (new_state, cooldown_ticks) = match obj.kind {
                arpg_core::InteractableKind::Chest
                | arpg_core::InteractableKind::Barrel
                | arpg_core::InteractableKind::Urn => (St::Destroyed, 0),
                arpg_core::InteractableKind::Door => (St::OnRecharge, 10),
                arpg_core::InteractableKind::Shrine | arpg_core::InteractableKind::Well => {
                    (St::OnRecharge, 250)
                }
                _ => (St::InUse, 0),
            };
            obj.state = new_state;
            if cooldown_ticks > 0 {
                obj.cooldown_until = arpg_core::Tick(tick.0 + cooldown_ticks);
            }
            let _ = self.replication.touch(player_id);
            let key = self.next_event_key(arpg_core::EntityId(player_id.0 as u64));
            self.event_buffer.emit(
                key,
                GameEvent::ItemPickedUp(arpg_core::ItemId(target_id.0 as u128)),
            );
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
