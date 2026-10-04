use crate::command::{Admission, ClientCommand, CommandEnvelope};
use crate::item::ItemLocation;
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
    /// Character level, used by the XP pipeline (SPEC.md section 110).
    pub level: i64,
    /// Experience points (SPEC.md section 110).
    pub experience: u64,
    /// Active over-time potion effect (SPEC.md section 83): remaining
    /// fixed-point value and ticks, applied once per tick.
    pub active_regen: Option<(i64, i64, u32)>,
}

/// A monster in the world: position, life, lifecycle-able target of
/// combat. Monsters are driven by the AI brain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonsterState {
    pub entity: EntityId,
    pub pos: WorldPos,
    pub life: i64,
    /// Loot table dropped on death (SPEC.md sections 71-72).
    pub treasure_class: Option<crate::item::TreasureClass>,
    /// Datapack definition stats (SPEC.md sections 57, 112): fixed at
    /// spawn, never retroactively modified.
    pub damage: i64,
    pub speed_fp: i32,
    pub aggro_range: i32,
    pub experience: u64,
    pub ranged: bool,
    /// Damage type index (0 physical, 1 magic, 2 fire, 3 cold, 4
    /// lightning, 5 poison) used to resolve resistance (SPEC.md section 51).
    pub damage_type: u8,
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
    pub expired_items: u64,
    pub game_seed: [u8; 32],
}

impl GameState {
    pub fn seed(&self) -> [u8; 32] {
        self.game_seed
    }

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
    /// IR skill registry (SPEC.md section 36). Data cannot depend on sim,
    /// so full skill programs are registered on the instance.
    pub skills: BTreeMap<arpg_core::SkillId, crate::skill::SkillDefinition>,
    pub missiles: Vec<crate::missile::MissileInstance>,
    next_missile_entity: u64,
    pub monsters: BTreeMap<EntityId, MonsterState>,
    pub inventory: crate::inventory::InventorySystem,
    pub loot_roller: crate::loot::LootRoller,
    /// Drops queued by PendingDeathResolution, consumed by LootResolution.
    pending_drops: Vec<(
        EntityId,
        WorldPos,
        [u8; 32],
        u16,
        crate::item::TreasureClass,
    )>,
    /// Born-tick of each ground item for expiration (SPEC.md section 88).
    ground_spawn_ticks: BTreeMap<arpg_core::ItemId, u64>,
    monster_move_intents: BTreeMap<EntityId, WorldPos>,
    pub ai_brain: Option<Box<dyn crate::ai::AiBrain>>,
    next_monster_entity: u64,
    /// Quest system: definitions + game/character states (SPEC.md 101-106).
    pub quests: crate::quest::QuestSystem,
    /// Waypoints unlocked per character x difficulty (SPEC.md section 107).
    pub waypoints: crate::quest::WaypointState,
    /// Town portals opened in this game (SPEC.md section 108).
    pub portals: crate::quest::PortalSystem,
    /// Quest events queued during earlier phases, consumed by
    /// QuestResolution (SPEC.md section 8 intra-tick visibility).
    pending_quest_events: Vec<crate::quest::QuestEvent>,
    /// Active level; when None, movement applies without terrain collision
    /// (used by tests and headless instances without a generated world).
    pub level: Option<LevelInstance>,
    scheduler: Scheduler,
    command_queue: CommandQueue,
    event_buffer: EventBuffer,
    pub replication: ReplicationTracker,
    /// Party management (SPEC.md section 109).
    pub parties: crate::social::PartySystem,
    /// Hostility declarations (SPEC.md sections 113-114).
    pub hostility: crate::social::HostilityMatrix,
    /// Summons and hirelings (SPEC.md sections 66-67).
    pub summons: crate::summon::SummonSystem,
    /// Sockets, runes and runewords (SPEC.md sections 76, 193, 199).
    pub sockets: crate::socket::SocketSystem,
    /// Auras (SPEC.md section 55): state producers refreshed on a
    /// data-owned interval.
    pub auras: crate::aura::AuraSystem,
    /// Active damage-over-time effects (SPEC.md section 53): keyed by
    /// (target, source) with a fixed-point accumulator.
    pub dots: BTreeMap<(EntityId, u64), crate::damage::DotAccumulator>,
    /// Two-player trades (SPEC.md sections 116-118).
    pub trades: crate::trade::TradeSystem,
    /// Gold and merchants (SPEC.md section 94).
    pub economy: crate::economy::Economy,
    /// Item sets (SPEC.md section 89): bonuses recomputed after each
    /// equipment transaction.
    pub sets: crate::set::SetSystem,
    /// Runtime metrics (SPEC.md section 190). POLICY domain: never part of
    /// the state hash, never alters gameplay. Optional so replays and
    /// tests can run without a collector.
    pub metrics: Option<std::sync::Arc<std::sync::Mutex<arpg_metrics::Metrics>>>,
    /// Combat and loot audit traces (SPEC.md sections 167-168). POLICY
    /// domain: never part of the state hash.
    pub traces: crate::trace::TraceBuffer,
    /// Effect states per entity (SPEC.md section 54): buffs, cures,
    /// resistance modifiers with durations.
    pub states: crate::states::StateStore,
    /// XP pipeline configuration (SPEC.md section 110).
    pub xp_pipeline: crate::social::XpPipeline,
}

pub struct TickResult {
    pub tick: Tick,
    pub state_hash: [u8; 32],
    pub events: Vec<GameEvent>,
    /// Commands executed this tick, post-scheduling (SPEC.md section
    /// 159): exactly what a replay records.
    pub executed_commands: Vec<ScheduledCommand>,
}

impl GameInstance {
    pub fn new(
        data: Arc<arpg_data::GameData>,
        rules: Arc<arpg_rules::GameRules>,
        seed: RootSeed,
    ) -> GameInstance {
        let game_seed = seed;
        GameInstance {
            data,
            rules,
            state: GameState {
                game_seed,
                ..GameState::default()
            },
            actors: BTreeMap::new(),
            skills: BTreeMap::new(),
            missiles: Vec::new(),
            next_missile_entity: 1 << 60,
            monsters: BTreeMap::new(),
            inventory: crate::inventory::InventorySystem::new(),
            loot_roller: crate::loot::LootRoller::new(),
            pending_drops: Vec::new(),
            ground_spawn_ticks: BTreeMap::new(),
            monster_move_intents: BTreeMap::new(),
            ai_brain: None,
            next_monster_entity: 2 << 40,
            quests: crate::quest::QuestSystem::new(),
            waypoints: crate::quest::WaypointState::default(),
            portals: crate::quest::PortalSystem::new(),
            pending_quest_events: Vec::new(),
            level: None,
            scheduler: Scheduler::new(),
            command_queue: CommandQueue::new(),
            event_buffer: EventBuffer::new(),
            replication: ReplicationTracker::new(),
            parties: crate::social::PartySystem::new(),
            hostility: crate::social::HostilityMatrix::default(),
            summons: crate::summon::SummonSystem::new(),
            sockets: crate::socket::SocketSystem::new(),
            auras: crate::aura::AuraSystem::new(),
            dots: BTreeMap::new(),
            trades: crate::trade::TradeSystem::new(),
            economy: crate::economy::Economy::new(),
            sets: crate::set::SetSystem::new(),
            metrics: None,
            traces: crate::trace::TraceBuffer::new(),
            states: crate::states::StateStore::new(),
            xp_pipeline: crate::social::XpPipeline::D2_LIKE,
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
                level: 1,
                experience: 0,
                active_regen: None,
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
    /// Effect state ids (SPEC.md section 54): stable numeric ids so the
    /// canonical hash stays datapack-independent.
    pub const STATE_RESIST_FIRE: u32 = 1;
    pub const STATE_RESIST_COLD: u32 = 2;
    pub const STATE_RESIST_LIGHTNING: u32 = 3;
    pub const STATE_RESIST_POISON: u32 = 4;
    pub const STATE_RESIST_MAGIC: u32 = 5;
    pub const STATE_POISONED: u32 = 6;
    pub const STATE_FROZEN: u32 = 7;
    pub const STATE_SLOWED: u32 = 8;

    /// Arrival-side admission bound (SPEC.md sections 170-171): a flood of
    /// commands must never cause unbounded allocation. Overflow increments
    /// the scheduler_overflow invariant counter (section 191).
    pub const MAX_QUEUED_COMMANDS: usize = 1024;

    pub fn submit_command(&mut self, envelope: CommandEnvelope) {
        if self.command_queue.len() >= Self::MAX_QUEUED_COMMANDS {
            if let Some(m) = &self.metrics {
                m.lock()
                    .unwrap()
                    .incr(arpg_metrics::names::SCHEDULER_OVERFLOW);
            }
            return;
        }
        self.command_queue.push(envelope);
    }

    /// Register an IR skill program; invalid skills are rejected (SPEC.md
    /// section 37).
    pub fn register_skill(
        &mut self,
        def: crate::skill::SkillDefinition,
    ) -> Result<(), crate::skill::SkillValidationError> {
        def.validate()?;
        self.skills.insert(def.id, def);
        Ok(())
    }

    /// Translate the datapack's skill programs (arpg-data
    /// SkillProgramData) into typed IR and register them. Called once at
    /// startup so classes and shared attacks come from the datapack
    /// (SPEC.md sections 35-37, 151).
    pub fn register_datapack_skills(&mut self) -> Result<(), crate::skill::SkillValidationError> {
        let defs: Vec<crate::skill::SkillDefinition> = self
            .data
            .skills
            .values()
            .filter_map(translate_skill)
            .collect();
        for def in defs {
            self.register_skill(def)?;
        }
        Ok(())
    }

    /// Plug an AI brain; it is consulted during Perception/AiDecision.
    pub fn set_ai_brain(&mut self, brain: Box<dyn crate::ai::AiBrain>) {
        self.ai_brain = Some(brain);
    }

    /// Spawn a monster and notify the brain (SPEC.md section 60).
    pub fn spawn_monster(&mut self, home: WorldPos) -> EntityId {
        self.spawn_monster_with_tc(home, None)
    }

    pub fn spawn_monster_with_tc(
        &mut self,
        home: WorldPos,
        treasure_class: Option<crate::item::TreasureClass>,
    ) -> EntityId {
        self.spawn_monster_internal(MonsterState {
            entity: EntityId(0),
            pos: home,
            life: 50,
            treasure_class,
            damage: 10,
            speed_fp: 256,
            aggro_range: 6,
            experience: 20,
            ranged: false,
            damage_type: 0,
        })
    }

    /// Spawn a monster from its datapack definition (SPEC.md sections 57,
    /// 112): stats are fixed at spawn from the definition.
    pub fn spawn_monster_def(
        &mut self,
        def_id: arpg_core::MonsterDefId,
        home: WorldPos,
    ) -> EntityId {
        let def = self.data.monsters.get(&def_id).cloned();
        let (life, damage, speed_fp, aggro_range, experience, ranged, damage_type) = match def {
            Some(d) => (
                d.base_life,
                d.damage,
                d.speed_fp,
                d.aggro_range,
                d.experience,
                d.ranged,
                d.damage_type,
            ),
            None => (50, 10, 256, 6, 20, false, 0),
        };
        self.spawn_monster_internal(MonsterState {
            entity: EntityId(0),
            pos: home,
            life,
            treasure_class: None,
            damage,
            speed_fp,
            aggro_range,
            experience,
            ranged,
            damage_type,
        })
    }

    fn spawn_monster_internal(&mut self, mut template: MonsterState) -> EntityId {
        self.next_monster_entity += 1;
        let entity = EntityId(self.next_monster_entity);
        template.entity = entity;
        let home = template.pos;
        self.monsters.insert(entity, template);
        self.actors.insert(entity, crate::actor::Actor::new(entity));
        if let Some(brain) = self.ai_brain.as_mut() {
            brain.on_spawn(entity, home);
        }
        let key = self.next_event_key(entity);
        self.event_buffer
            .emit(key, GameEvent::EntitySpawned(entity));
        entity
    }

    pub fn tick(&mut self) -> TickResult {
        let started = std::time::Instant::now();
        self.state.tick = self.state.tick.next();
        let tick = self.state.tick;

        let ingested = self.command_queue.drain();
        let mut scheduled: Vec<ScheduledCommand> = Vec::new();
        for envelope in ingested {
            tracing::trace!(
                tick = tick.0,
                player_identity_id = envelope.player.0,
                command_sequence = envelope.sequence,
                "command received"
            );
            let admission = self
                .scheduler
                .admit(envelope, tick, DEFAULT_INPUT_DELAY_TICKS);
            if let Some(metrics) = &self.metrics {
                let mut m = metrics.lock().unwrap();
                m.incr(arpg_metrics::names::COMMANDS_RECEIVED);
                match admission {
                    Admission::Accepted => m.incr(arpg_metrics::names::COMMANDS_ACCEPTED),
                    Admission::Duplicate => m.incr(arpg_metrics::names::COMMANDS_REJECTED),
                    Admission::RejectedTooOld => m.incr(arpg_metrics::names::COMMANDS_REJECTED),
                    Admission::RejectedInvalid => m.incr(arpg_metrics::names::COMMANDS_REJECTED),
                    Admission::Deferred => {}
                }
            }
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
        for (id, m) in &self.monsters {
            hash_input.extend_from_slice(&id.0.to_le_bytes());
            hash_input.extend_from_slice(&m.pos.x.to_le_bytes());
            hash_input.extend_from_slice(&m.pos.y.to_le_bytes());
            hash_input.extend_from_slice(&m.life.to_le_bytes());
        }
        for m in &self.missiles {
            hash_input.extend_from_slice(&m.entity.0.to_le_bytes());
            hash_input.extend_from_slice(&m.position.x.to_le_bytes());
            hash_input.extend_from_slice(&m.position.y.to_le_bytes());
            hash_input.extend_from_slice(&m.lifetime.to_le_bytes());
            hash_input.extend_from_slice(&(m.hit_entities.len() as u64).to_le_bytes());
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
        for (id, loc) in self.inventory.iter_locations() {
            hash_input.extend_from_slice(&id.0.to_le_bytes());
            match loc {
                ItemLocation::PlayerInventory(p, g) => {
                    hash_input.extend_from_slice(&p.0.to_le_bytes());
                    hash_input.push(0);
                    hash_input.push(g.x);
                    hash_input.push(g.y);
                }
                ItemLocation::Equipment(p, s) => {
                    hash_input.extend_from_slice(&p.0.to_le_bytes());
                    hash_input.push(1);
                    hash_input.push(*s as u8);
                }
                ItemLocation::Belt(p, s) => {
                    hash_input.extend_from_slice(&p.0.to_le_bytes());
                    hash_input.push(2);
                    hash_input.push(*s);
                }
                ItemLocation::Stash(p, s) => {
                    hash_input.extend_from_slice(&p.0.to_le_bytes());
                    hash_input.push(3);
                    hash_input.extend_from_slice(&s.page.to_le_bytes());
                    hash_input.push(s.x);
                    hash_input.push(s.y);
                }
                ItemLocation::Cube(p, g) => {
                    hash_input.extend_from_slice(&p.0.to_le_bytes());
                    hash_input.push(4);
                    hash_input.push(g.x);
                    hash_input.push(g.y);
                }
                ItemLocation::Ground(l, pos) => {
                    hash_input.extend_from_slice(&l.0.to_le_bytes());
                    hash_input.push(5);
                    hash_input.extend_from_slice(&pos.x.to_le_bytes());
                    hash_input.extend_from_slice(&pos.y.to_le_bytes());
                }
            }
        }
        hash_input.extend_from_slice(&self.state.expired_items.to_le_bytes());
        hash_input.extend_from_slice(&self.scheduler.canonical_hash_input());
        self.parties.hash_bytes(&mut hash_input);
        self.hostility.hash_bytes(&mut hash_input);
        self.summons.hash_bytes(&mut hash_input);
        self.sockets.hash_bytes(&mut hash_input);
        self.auras.hash_bytes(&mut hash_input);
        for (key, dot) in &self.dots {
            hash_input.extend_from_slice(&key.0 .0.to_le_bytes());
            hash_input.extend_from_slice(&key.1.to_le_bytes());
            hash_input.extend_from_slice(&dot.total_damage_fp.to_le_bytes());
            hash_input.extend_from_slice(&(dot.remaining_ticks as i64).to_le_bytes());
            hash_input.extend_from_slice(&dot.accumulator.to_le_bytes());
        }
        hash_input.extend_from_slice(&self.states.canonical_hash_input());
        let state_hash = arpg_core::hash::state_hash(&hash_input);
        if let Some(metrics) = &self.metrics {
            let mut m = metrics.lock().unwrap();
            m.observe_duration(arpg_metrics::names::GAME_TICK_SECONDS, started.elapsed());
            m.set_gauge(
                arpg_metrics::names::GAME_ENTITY_COUNT,
                self.actors.len() as f64,
            );
            m.set_gauge(
                arpg_metrics::names::GAME_MONSTER_COUNT,
                self.monsters.len() as f64,
            );
            m.set_gauge(
                arpg_metrics::names::GAME_MISSILE_COUNT,
                self.missiles.len() as f64,
            );
        }

        TickResult {
            tick,
            state_hash,
            events,
            executed_commands: due,
        }
    }

    fn run_phase(
        &mut self,
        phase: Phase,
        tick: Tick,
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
                    ClientCommand::UseSkill(intent)
                        if self.state.players.contains_key(&cmd.player) =>
                    {
                        self.start_cast(cmd.player, intent.skill, intent.target, tick);
                    }
                    ClientCommand::UseItem(intent)
                        if self.state.players.contains_key(&cmd.player) =>
                    {
                        self.use_potion(cmd.player, intent.item);
                    }
                    ClientCommand::Trade(intent)
                        if self.state.players.contains_key(&cmd.player) =>
                    {
                        self.handle_trade(cmd.player, intent.clone(), tick);
                    }
                    ClientCommand::Merchant(intent)
                        if self.state.players.contains_key(&cmd.player) =>
                    {
                        self.handle_merchant(cmd.player, intent.clone());
                    }
                    _ => {}
                }
            }
        }

        if let Phase::Perception = phase {
            self.ai_perceive();
        }

        if let Phase::Regeneration = phase {
            self.apply_potion_regen();
        }

        if let Phase::EffectGeneration = phase {
            self.apply_auras(tick);
        }
        if let Phase::PeriodicStates = phase {
            self.apply_dots();
            self.states.expire(tick);
        }

        if let Phase::AiDecision = phase {
            self.ai_decide(tick);
        }

        if let Phase::ActionStateAdvance = phase {
            self.advance_actions(tick);
            self.resolve_cast_impacts();
        }

        if let Phase::PendingDeathResolution = phase {
            self.resolve_pending_deaths();
        }

        if let Phase::MovementResolution = phase {
            self.resolve_movement();
            self.state.movement_intents.clear();
            self.resolve_monster_moves();
        }

        if let Phase::InteractionResolution = phase {
            self.resolve_interactions(tick);
            self.state.interact_intents.clear();
        }

        if let Phase::LootResolution = phase {
            self.resolve_loot();
        }

        if let Phase::QuestResolution = phase {
            self.resolve_quests();
        }
        if let Phase::Expiration = phase {
            self.resolve_expiration();
        }

        if let Phase::MissileMovement = phase {
            self.advance_missiles();
        }

        if let Phase::MissileCollision = phase {
            self.resolve_missile_collisions();
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

    /// Perception phase (SPEC.md section 60): build the read-only world
    /// view for the brain.
    fn ai_perceive(&mut self) {
        // nothing to do: the view is built lazily in ai_decide
    }

    /// AiDecision phase (SPEC.md section 60): the brain produces commands;
    /// the sim applies them. Monsters never mutate the world directly.
    fn ai_decide(&mut self, tick: arpg_core::Tick) {
        let Some(brain) = self.ai_brain.as_mut() else {
            return;
        };
        if self.monsters.is_empty() {
            return;
        }
        let view = crate::ai::AiWorldView {
            players: self
                .state
                .players
                .values()
                .map(|p| (EntityId(p.player.0 as u64), p.pos))
                .collect(),
            monsters: self.monsters.values().map(|m| (m.entity, m.pos)).collect(),
            monster_stats: self
                .monsters
                .values()
                .map(|m| {
                    (
                        m.entity,
                        crate::ai::MonsterPerception {
                            aggro_range_fp: m.aggro_range as i64 * 256,
                            ranged: m.ranged,
                        },
                    )
                })
                .collect(),
        };
        let commands = brain.think(tick, &view);
        for (entity, command) in commands {
            match command {
                crate::ai::AiCommand::MoveTo(dest) => {
                    self.monster_move_intents.insert(entity, dest);
                }
                crate::ai::AiCommand::Attack(target) => {
                    self.monster_attack(entity, target);
                }
            }
        }
    }

    /// A monster attacks a player: melee-range damage applied through the
    /// normal damage path so kill credit and PendingDeath work (SPEC.md
    /// sections 13-14, 46).
    /// Drink a potion (SPEC.md sections 82-83): the item must exist in the
    /// player's inventory or belt; its effect applies instantly or arms an
    /// over-time regen consumed one tick later per tick. The potion is
    /// consumed on use.
    pub fn use_potion(&mut self, player: PlayerId, item: arpg_core::ItemId) -> bool {
        let Some(def_id) = self.inventory.get(item).map(|i| i.definition) else {
            return false;
        };
        let Some(def) = self.data.items.get(&def_id.0) else {
            return false;
        };
        let Some(effect) = def.potion else {
            return false;
        };
        match effect {
            arpg_data::PotionEffect::Instant { life_fp, mana_fp } => {
                if let Some(p) = self.state.players.get_mut(&player) {
                    p.life += crate::damage::fixed_to_units(life_fp);
                    p.mana += crate::damage::fixed_to_units(mana_fp);
                }
            }
            arpg_data::PotionEffect::OverTime {
                life_fp,
                mana_fp,
                ticks,
            } => {
                if let Some(p) = self.state.players.get_mut(&player) {
                    p.active_regen = Some((life_fp, mana_fp, ticks.max(1)));
                }
            }
            arpg_data::PotionEffect::Resistance {
                kind,
                percent,
                ticks,
            } => {
                // resistance modifier state with a duration (sections 54, 83)
                let state_id = match kind {
                    arpg_data::ResistKind::Fire => Self::STATE_RESIST_FIRE,
                    arpg_data::ResistKind::Cold => Self::STATE_RESIST_COLD,
                    arpg_data::ResistKind::Lightning => Self::STATE_RESIST_LIGHTNING,
                    arpg_data::ResistKind::Poison => Self::STATE_RESIST_POISON,
                    arpg_data::ResistKind::Magic => Self::STATE_RESIST_MAGIC,
                };
                let entity = EntityId(player.0 as u64);
                let instance = crate::states::StateInstance {
                    state: state_id,
                    source: entity,
                    source_skill: None,
                    applied_tick: self.state.tick,
                    expires_tick: Some(Tick(self.state.tick.0 + ticks as u64)),
                    stack_key: (state_id, entity.0),
                    magnitude_bp: (percent * 100) as i32,
                };
                self.states
                    .apply(entity, instance, crate::states::StackPolicy::Refresh);
            }
            arpg_data::PotionEffect::Cure { kind } => {
                // remove the matching detrimental state (sections 54, 83)
                let entity = EntityId(player.0 as u64);
                let cured = match kind {
                    arpg_data::CureKind::Poison => Self::STATE_POISONED,
                    arpg_data::CureKind::Cold => Self::STATE_FROZEN,
                    arpg_data::CureKind::Stamina => Self::STATE_SLOWED,
                };
                self.states.remove(entity, cured, entity);
            }
        }
        self.inventory.remove(item);
        let key = self.next_event_key(EntityId(player.0 as u64));
        self.event_buffer.emit(key, GameEvent::ItemPickedUp(item));
        true
    }

    /// Apply one tick of active over-time potion regen (section 83).
    /// Refresh auras (SPEC.md section 55): each due aura applies its state
    /// to entities inside its radius per the target filter; aura states
    /// stack under Refresh policy keyed by the aura owner as source.
    fn apply_auras(&mut self, tick: Tick) {
        if self.auras.active.is_empty() {
            return;
        }
        let due: Vec<(EntityId, u32, crate::aura::AuraDefinition)> = self
            .auras
            .due(tick)
            .into_iter()
            .map(|(o, a, d)| (o, a.definition, d.clone()))
            .collect();
        for (owner, definition, def) in due {
            let Some(owner_pos) = self.entity_pos(owner) else {
                self.auras.deactivate(owner);
                continue;
            };
            let targets: Vec<EntityId> = self
                .state
                .players
                .values()
                .map(|p| (EntityId(p.player.0 as u64), p.pos))
                .filter(|(e, _)| {
                    *e == owner
                        || matches!(
                            def.target_filter,
                            crate::aura::TargetFilter::Party
                                | crate::aura::TargetFilter::Allies
                                | crate::aura::TargetFilter::Everyone
                        )
                })
                .filter(|(_, pos)| crate::aura::within_radius(owner_pos, *pos, def.radius_fp))
                .map(|(e, _)| e)
                .collect();
            let instance = crate::aura::AuraInstance {
                definition,
                owner,
                last_refresh: tick,
            };
            self.auras.active.insert(owner, instance);
            for target in targets {
                let state_instance = crate::states::StateInstance {
                    state: def.state,
                    source: owner,
                    source_skill: None,
                    applied_tick: tick,
                    expires_tick: Some(Tick(tick.0 + def.duration_ticks as u64)),
                    stack_key: (def.state, owner.0),
                    magnitude_bp: def.magnitude_bp,
                };
                self.states
                    .apply(target, state_instance, crate::states::StackPolicy::Refresh);
            }
        }
    }

    /// Handle a trade intent (SPEC.md sections 116-118): open, set
    /// offer, accept or cancel. The commit is persistence-driven: the
    /// caller injects the store, and `TradeCommitted` semantics stay
    /// owned by the TradeSystem.
    fn handle_trade(&mut self, player: PlayerId, intent: crate::command::TradeIntent, _tick: Tick) {
        use crate::command::TradeIntent;
        match intent {
            TradeIntent::Open { target } => {
                if self.state.players.contains_key(&target) && target != player {
                    self.trades.open(player, target);
                }
            }
            TradeIntent::SetOffer { trade, items, gold } => {
                let _ =
                    self.trades
                        .set_offer(arpg_persistence::TradeId(trade), player, items, gold);
            }
            TradeIntent::Accept { trade } => {
                let _ = self.trades.accept(arpg_persistence::TradeId(trade), player);
            }
            TradeIntent::Cancel { trade } => {
                let _ = self.trades.cancel(arpg_persistence::TradeId(trade));
            }
        }
    }

    /// Commit a fully accepted trade against a persistence store
    /// (SPEC.md sections 117-118). Returns the commit outcome.
    pub fn commit_trade<S: arpg_persistence::PersistenceStore>(
        &mut self,
        trade: u64,
        revisions: &std::collections::BTreeMap<PlayerId, arpg_persistence::CharacterRevision>,
        store: &mut S,
    ) -> Result<Result<(), arpg_persistence::PersistenceError>, crate::trade::TradeError> {
        self.trades.commit(
            arpg_persistence::TradeId(trade),
            &mut self.economy,
            &mut self.inventory,
            revisions,
            store,
        )
    }

    /// Handle a merchant intent (SPEC.md sections 95-97): buy, sell,
    /// repair, gamble against the player's personal stock. Prices come
    /// from the merchant's stock entries; failures are silently ignored
    /// as commands describe intentions, never results (INV-009).
    fn handle_merchant(&mut self, player: PlayerId, intent: crate::command::MerchantIntent) {
        use crate::command::MerchantIntent;
        match intent {
            MerchantIntent::Buy {
                merchant,
                def,
                price,
            } => {
                let mut gold = self.economy.gold_of(player);
                let bought = self
                    .economy
                    .merchant(merchant)
                    .buy(player, &mut gold, def, price);
                if bought.is_ok() {
                    self.economy.gold.insert(player, gold);
                    self.give_item_to(player, def);
                }
            }
            MerchantIntent::Sell {
                merchant,
                item,
                base_price,
            } => {
                // ownership: the item must be in the seller's inventory
                let owned = self
                    .inventory
                    .location(item)
                    .is_some_and(|loc| matches!(loc, crate::item::ItemLocation::PlayerInventory(p, _) if p == player));
                if !owned {
                    return;
                }
                let mut gold = self.economy.gold_of(player);
                let payout = self
                    .economy
                    .merchant(merchant)
                    .sell(player, &mut gold, base_price);
                self.economy.gold.insert(player, gold);
                let _ = payout;
                self.inventory.remove(item);
            }
            MerchantIntent::Repair { merchant, item } => {
                let target = item.or_else(|| {
                    self.inventory
                        .iter_locations()
                        .find(|(id, loc)| {
                            matches!(loc, crate::item::ItemLocation::PlayerInventory(p, _) if p == &player)
                                && self
                                    .inventory
                                    .get(*id)
                                    .and_then(|i| i.durability)
                                    .is_some()
                        })
                        .map(|(id, _)| id)
                });
                let Some(target) = target else { return };
                let missing = self
                    .inventory
                    .get(target)
                    .and_then(|i| i.durability)
                    .map(|d| (100u64).saturating_sub(d as u64))
                    .unwrap_or(0);
                if missing == 0 {
                    return;
                }
                let cost = self.economy.merchant(merchant).repair_cost(missing);
                let mut gold = self.economy.gold_of(player);
                if gold.carried >= cost {
                    gold.carried -= cost;
                    self.economy.gold.insert(player, gold);
                    self.inventory.set_durability(target, 100);
                }
            }
            MerchantIntent::Gamble { merchant, offer } => {
                let mut gold = self.economy.gold_of(player);
                let result =
                    self.economy
                        .merchant(merchant)
                        .gamble_buy(player, &mut gold, offer as usize);
                if let Ok(instance) = result {
                    self.economy.gold.insert(player, gold);
                    let id = arpg_core::ItemId(self.inventory.highest_item_id() + 1);
                    let instance = crate::item::ItemInstance { id, ..instance };
                    self.inventory.spawn_ground(
                        instance,
                        arpg_core::LevelInstanceId(0),
                        self.state
                            .players
                            .get(&player)
                            .map(|p| p.pos)
                            .unwrap_or(WorldPos::ZERO),
                    );
                }
            }
        }
    }

    /// Give a freshly bought item to a player: spawned at their feet so
    /// pickup rules apply (SPEC.md section 86).
    fn give_item_to(&mut self, player: PlayerId, def: arpg_core::ItemDefId) {
        let id = arpg_core::ItemId(self.inventory.highest_item_id() + 1);
        let instance = crate::item::ItemInstance {
            id,
            definition: def,
            quality: crate::item::ItemQuality::Normal,
            item_level: 1,
            generation_seed: [0; 32],
            affixes: smallvec::SmallVec::new(),
            sockets: smallvec::SmallVec::new(),
            durability: None,
            flags: 0,
        };
        let pos = self
            .state
            .players
            .get(&player)
            .map(|p| p.pos)
            .unwrap_or(WorldPos::ZERO);
        self.inventory
            .spawn_ground(instance, arpg_core::LevelInstanceId(0), pos);
    }

    /// Position of an entity (player or monster), canonical lookup.
    fn entity_pos(&self, entity: EntityId) -> Option<WorldPos> {
        if let Some(p) = self
            .state
            .players
            .values()
            .find(|p| EntityId(p.player.0 as u64) == entity)
        {
            return Some(p.pos);
        }
        self.monsters.get(&entity).map(|m| m.pos)
    }

    fn apply_potion_regen(&mut self) {
        let players: Vec<PlayerId> = self.state.players.keys().copied().collect();
        for player in players {
            let Some((life_fp, mana_fp, ticks)) =
                self.state.players.get(&player).and_then(|p| p.active_regen)
            else {
                continue;
            };
            let per_tick_life = life_fp / ticks as i64;
            let per_tick_mana = mana_fp / ticks as i64;
            if let Some(p) = self.state.players.get_mut(&player) {
                p.life += crate::damage::fixed_to_units(per_tick_life);
                p.mana += crate::damage::fixed_to_units(per_tick_mana);
                if ticks <= 1 {
                    p.active_regen = None;
                } else {
                    p.active_regen =
                        Some((life_fp - per_tick_life, mana_fp - per_tick_mana, ticks - 1));
                }
            }
        }
    }

    pub fn monster_attack(&mut self, monster: EntityId, target: EntityId) {
        let Some(m) = self.monsters.get(&monster) else {
            return;
        };
        let resists = self.effective_resists(target);
        let Some(p) = self
            .state
            .players
            .values()
            .find(|p| EntityId(p.player.0 as u64) == target)
        else {
            return;
        };
        let within_range = m.pos.dist2(p.pos) <= (2 * 256) * (2 * 256);
        if !within_range {
            return;
        }
        // Hit resolution (SPEC.md sections 46-47): ruleset CTH, roll drawn
        // from a dedicated BLAKE3 combat domain, trace recorded (167).
        let attack_rating = m.damage * 100;
        let defense = p.level * 25;
        let attacker_level = (m.damage / 10).max(1);
        let chance_bp =
            arpg_rules::chance_to_hit(attack_rating, defense, attacker_level, p.level) * 100;
        let roll_bp = (self.combat_draw(monster, target) % 9500) as i64;
        let hit = roll_bp < chance_bp;
        let amount = m.damage;
        let tick = self.state.tick;
        let attack_index = self.traces.attack_index() + 1;
        let (final_amount, resist_percent) = if hit {
            if m.damage_type == 5 {
                // Poison (section 53): a hit applies a DoT, not instant
                // damage; the poison resistance scales the DoT total.
                let resolved = crate::damage::apply_resist(amount, resists.poison_bp);
                (resolved, self.resist_percent(&resists, m.damage_type))
            } else {
                let resolved = crate::damage::resolve_damage(
                    crate::damage::RollAmounts {
                        physical: if m.damage_type == 0 { amount } else { 0 },
                        magic: if m.damage_type == 1 { amount } else { 0 },
                        fire: if m.damage_type == 2 { amount } else { 0 },
                        cold: if m.damage_type == 3 { amount } else { 0 },
                        lightning: if m.damage_type == 4 { amount } else { 0 },
                    },
                    &resists,
                );
                (resolved, self.resist_percent(&resists, m.damage_type))
            }
        } else {
            (0, 0)
        };
        self.traces.record_attack(crate::trace::AttackTrace {
            tick,
            attack_index,
            source: monster,
            target,
            attack_rating,
            defense,
            chance_bp,
            roll_bp,
            hit,
            physical_raw: amount,
            resistance_percent: resist_percent,
            final_damage: final_amount,
        });
        if hit {
            if m.damage_type == 5 {
                let total_fp = final_amount.max(0) * 256;
                let key = (target, monster.0);
                match self.dots.get_mut(&key) {
                    Some(dot) if dot.remaining_ticks > 0 => {
                        dot.total_damage_fp = dot.total_damage_fp.saturating_add(total_fp);
                    }
                    _ => {
                        self.dots
                            .insert(key, crate::damage::DotAccumulator::new(total_fp, 10));
                    }
                }
                let instance = crate::states::StateInstance {
                    state: Self::STATE_POISONED,
                    source: monster,
                    source_skill: None,
                    applied_tick: tick,
                    expires_tick: Some(Tick(tick.0 + 10)),
                    stack_key: (Self::STATE_POISONED, monster.0),
                    magnitude_bp: 0,
                };
                self.states
                    .apply(target, instance, crate::states::StackPolicy::Refresh);
            } else if final_amount > 0 {
                self.apply_damage(target, monster, final_amount);
            }
        }
    }

    /// Apply one tick of every active DoT (SPEC.md section 53): the
    /// accumulator guarantees the total paid is exact regardless of
    /// rounding; a finished DoT is removed.
    fn apply_dots(&mut self) {
        if self.dots.is_empty() {
            return;
        }
        let keys: Vec<(EntityId, u64)> = self.dots.keys().copied().collect();
        for key in keys {
            let Some(dot) = self.dots.get_mut(&key) else {
                continue;
            };
            let amount_fp = dot.tick_amount();
            let amount = crate::damage::fixed_to_units(amount_fp);
            let remaining = dot.remaining_ticks;
            let source = EntityId(key.1);
            if amount > 0 {
                self.apply_damage(key.0, source, amount);
            }
            if remaining == 0 {
                self.dots.remove(&key);
            }
        }
    }

    /// Effective per-type resistances of an entity (SPEC.md sections 51,
    /// 54): base resistances plus active resistance states, capped at the
    /// immunity threshold.
    pub fn effective_resists(&self, entity: EntityId) -> crate::damage::Resistances {
        let mut resists = crate::damage::Resistances::default();
        for s in self.states.entity_states(entity) {
            match s.state {
                Self::STATE_RESIST_FIRE => {
                    resists.fire_bp = (resists.fire_bp + s.magnitude_bp).min(10_000)
                }
                Self::STATE_RESIST_COLD => {
                    resists.cold_bp = (resists.cold_bp + s.magnitude_bp).min(10_000)
                }
                Self::STATE_RESIST_LIGHTNING => {
                    resists.lightning_bp = (resists.lightning_bp + s.magnitude_bp).min(10_000)
                }
                Self::STATE_RESIST_POISON => {
                    resists.poison_bp = (resists.poison_bp + s.magnitude_bp).min(10_000)
                }
                Self::STATE_RESIST_MAGIC => {
                    resists.magic_bp = (resists.magic_bp + s.magnitude_bp).min(10_000)
                }
                _ => {}
            }
        }
        resists
    }

    fn resist_percent(&self, resists: &crate::damage::Resistances, damage_type: u8) -> i64 {
        let bp = match damage_type {
            0 => resists.physical_bp,
            1 => resists.magic_bp,
            2 => resists.fire_bp,
            3 => resists.cold_bp,
            4 => resists.lightning_bp,
            _ => resists.poison_bp,
        };
        (bp as i64) / 100
    }

    /// Deterministic combat roll (SPEC.md section 47): BLAKE3(game seed ||
    /// attacker || defender || tick || attack counter), a dedicated domain
    /// like the loot drop seed (section 71).
    fn combat_draw(&self, attacker: EntityId, defender: EntityId) -> u64 {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&self.state.seed());
        hasher.update(&attacker.0.to_le_bytes());
        hasher.update(&defender.0.to_le_bytes());
        hasher.update(&self.state.tick.0.to_le_bytes());
        hasher.update(&self.traces.attack_index().to_le_bytes());
        let out = *hasher.finalize().as_bytes();
        u64::from_le_bytes(out[..8].try_into().unwrap())
    }

    /// Apply AI movement intents recorded during AiDecision, in canonical
    /// entity order, with entity-collision like players.
    fn resolve_monster_moves(&mut self) {
        if self.monster_move_intents.is_empty() {
            return;
        }
        let intents: Vec<(EntityId, WorldPos)> = self
            .monster_move_intents
            .iter()
            .map(|(e, d)| (*e, *d))
            .collect();
        self.monster_move_intents.clear();
        for (entity, dest) in intents {
            let Some(m) = self.monsters.get(&entity) else {
                continue;
            };
            let from = m.pos;
            if from == dest {
                continue;
            }
            if let Some(level) = &self.level {
                if !level.walkable_at(dest) {
                    continue;
                }
            }
            // one tile per tick toward the destination, axis-aligned
            let dx = (dest.x - from.x).signum() * 256;
            let dy = (dest.y - from.y).signum() * 256;
            let next = WorldPos::new(from.x + dx, from.y + dy);
            let occupied = self
                .state
                .players
                .values()
                .any(|p| p.pos.tile() == next.tile())
                || self
                    .monsters
                    .values()
                    .any(|other| other.entity != entity && other.pos.tile() == next.tile());
            if occupied {
                continue;
            }
            let Some(m) = self.monsters.get_mut(&entity) else {
                continue;
            };
            m.pos = next;
        }
    }

    /// Start a Cast action for a player's skill (SPEC.md section 32).
    fn start_cast(
        &mut self,
        player: PlayerId,
        skill: arpg_core::SkillId,
        target: Option<WorldPos>,
        tick: Tick,
    ) {
        let timing = match self.skills.get(&skill) {
            Some(def) => match def.timing {
                crate::skill::TimingFormula::Ticks(t) => crate::actor::ActionTiming {
                    windup_ticks: t,
                    impact_tick: t,
                    recovery_ticks: t,
                },
                crate::skill::TimingFormula::Instant => crate::actor::ActionTiming {
                    windup_ticks: 1,
                    impact_tick: 1,
                    recovery_ticks: 0,
                },
            },
            None => return,
        };
        let target_kind = match target {
            Some(pos) => crate::actor::Target::Position(pos),
            None => crate::actor::Target::None,
        };
        let entity = EntityId(player.0 as u64);
        if let Some(actor) = self.actors.get_mut(&entity) {
            actor.start_action(arpg_core::ActorMode::Cast, tick, target_kind, timing);
            if let Some(a) = actor.action.as_mut() {
                a.skill = Some(skill);
            }
        }
    }

    /// When a Cast action reaches Impact, execute its skill program
    /// (SPEC.md sections 36, 46).
    fn resolve_cast_impacts(&mut self) {
        let mut outcomes: Vec<(EntityId, arpg_core::ActorMode, EntityId)> = Vec::new();
        for (id, actor) in self.actors.iter_mut() {
            if let Some(action) = &mut actor.action {
                if actor.mode == arpg_core::ActorMode::Cast
                    && action.phase == arpg_core::ActionPhase::Impact
                {
                    // re-arm so we don't fire twice
                    action.phase = arpg_core::ActionPhase::Recovery;
                    outcomes.push((*id, action.mode, *id));
                }
            }
        }
        for (caster, _, _) in outcomes {
            self.execute_skill(caster);
        }
    }

    /// Execute the skill program of a caster's pending cast.
    fn execute_skill(&mut self, caster: EntityId) {
        let Some(player_id) = self
            .state
            .players
            .keys()
            .find(|p| EntityId(p.0 as u64) == caster)
            .copied()
        else {
            return;
        };
        let skill_id = {
            let Some(actor) = self.actors.get(&caster) else {
                return;
            };
            let Some(action) = &actor.action else {
                return;
            };
            action.skill.unwrap_or(arpg_core::SkillId(0))
        };
        let Some(def) = self.skills.get(&skill_id).cloned() else {
            return;
        };
        let target_pos = self
            .actors
            .get(&caster)
            .and_then(|a| a.action.as_ref())
            .and_then(|a| match a.target {
                crate::actor::Target::Position(p) => Some(p),
                _ => None,
            });
        let intent = crate::command::UseSkillIntent {
            skill: skill_id,
            target: target_pos,
        };
        let caster_pos = self
            .state
            .players
            .get(&player_id)
            .map(|p| p.pos)
            .unwrap_or(WorldPos::ZERO);
        let mut spawned: Vec<u32> = Vec::new();
        let outcome = crate::skill::execute_program(&def.program, &intent, |m| spawned.push(m));
        for def_id in spawned {
            self.spawn_missile(def_id, caster, caster_pos, target_pos);
        }
        if outcome.damage != 0 || outcome.heal != 0 || outcome.mana_restored != 0 {
            self.apply_skill_outcome(caster, target_pos, &outcome);
        }
    }

    fn apply_skill_outcome(
        &mut self,
        caster: EntityId,
        target_pos: Option<WorldPos>,
        outcome: &crate::skill::SkillOutcome,
    ) {
        let target = target_pos.and_then(|pos| {
            self.state
                .players
                .values()
                .find(|p| p.pos == pos && EntityId(p.player.0 as u64) != caster)
                .map(|p| EntityId(p.player.0 as u64))
                .or_else(|| {
                    // position-targeted skills hit the closest monster to the
                    // target position (ties resolved by entity id, canonical)
                    self.monsters
                        .values()
                        .filter(|m| m.entity != caster)
                        .min_by_key(|m| {
                            let dx = m.pos.x - pos.x;
                            let dy = m.pos.y - pos.y;
                            (dx * dx + dy * dy, m.entity.0)
                        })
                        .map(|m| m.entity)
                })
        });
        if let Some(target) = target {
            if outcome.damage != 0 {
                self.apply_damage(target, caster, outcome.damage);
            }
            if outcome.heal != 0 {
                if let Some(p) = self.state.players.get_mut(&PlayerId(target.0 as u32)) {
                    p.life = p.life.saturating_add(outcome.heal);
                }
            }
            if outcome.mana_restored != 0 {
                if let Some(p) = self.state.players.get_mut(&PlayerId(target.0 as u32)) {
                    p.mana = p.mana.saturating_add(outcome.mana_restored);
                }
            }
        }
    }

    fn spawn_missile(
        &mut self,
        definition: u32,
        owner: EntityId,
        from: WorldPos,
        to: Option<WorldPos>,
    ) {
        let velocity = match to {
            Some(dest) => {
                let dx = dest.x - from.x;
                let dy = dest.y - from.y;
                // one tile per tick toward the target, axis-aligned
                arpg_core::FixedVec2 {
                    x: dx.signum() * 256,
                    y: dy.signum() * 256,
                }
            }
            None => arpg_core::FixedVec2::ZERO,
        };
        self.next_missile_entity += 1;
        self.missiles.push(crate::missile::MissileInstance {
            entity: EntityId(self.next_missile_entity),
            definition,
            owner,
            source_skill: None,
            position: from,
            velocity,
            lifetime: 16,
            movement: crate::missile::MissileMovement::Linear,
            hit_entities: Vec::new(),
            remaining_pierces: 0,
        });
    }

    /// MissileMovement phase (SPEC.md section 57): move every missile; the
    /// return of advance() marks expiry.
    fn advance_missiles(&mut self) {
        let mut expired = Vec::new();
        for m in self.missiles.iter_mut() {
            if m.advance() {
                expired.push(m.entity);
            }
        }
        self.missiles.retain(|m| !expired.contains(&m.entity));
    }

    /// MissileCollision phase (SPEC.md section 58): a missile overlapping a
    /// player's tile hits it once.
    fn resolve_missile_collisions(&mut self) {
        let hits: Vec<(arpg_core::EntityId, arpg_core::EntityId)> = {
            let mut hits = Vec::new();
            for m in &self.missiles {
                for p in self.state.players.values() {
                    let target = EntityId(p.player.0 as u64);
                    if target != m.owner && m.hits_at(p.pos, 256) {
                        hits.push((m.entity, target));
                    }
                }
            }
            hits
        };
        for (missile_entity, target) in hits {
            let damage = 10i64;
            let Some(idx) = self
                .missiles
                .iter()
                .position(|m| m.entity == missile_entity)
            else {
                continue;
            };
            let destroyed = self.missiles[idx].register_hit(target);
            if destroyed {
                let owner = self.missiles[idx].owner;
                self.missiles.remove(idx);
                self.apply_damage(target, owner, damage);
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
            // queue the drop for LootResolution (SPEC.md sections 71, 88):
            // an object dropped at tick T is lootable from T+1 (section 10)
            if let Some(m) = self.monsters.get(&id) {
                if let Some(tc) = m.treasure_class.clone() {
                    let drop_seed = derive_drop_seed(self.state.seed(), id, self.state.tick);
                    self.pending_drops
                        .push((id, m.pos, drop_seed, self.state.tick.0 as u16, tc));
                }
            }
            self.monsters.remove(&id);
            let key = self.next_event_key(id);
            self.event_buffer.emit(key, GameEvent::EntityRemoved(id));
        }
    }

    /// Monster damage path: same lifecycle semantics as players (SPEC.md
    /// sections 13-14).
    fn apply_monster_damage(&mut self, target: EntityId, source: EntityId, amount: i64) {
        let (was_alive, now_dead) = {
            let Some(m) = self.monsters.get_mut(&target) else {
                return;
            };
            let was_alive = m.life > 0;
            m.life = m.life.saturating_sub(amount);
            (was_alive, m.life <= 0)
        };
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
            self.queue_quest_event(
                crate::quest::QuestTrigger::MonsterKilled(arpg_core::MonsterDefId(0)),
                source,
            );
            if was_alive && now_dead {
                self.award_monster_xp(target);
            }
        }
    }

    /// Multiplayer XP pipeline (SPEC.md section 110): participants are the
    /// alive players; party members share via the party distribution.
    fn award_monster_xp(&mut self, monster: EntityId) {
        let base_xp = self
            .monsters
            .get(&monster)
            .map(|m| m.experience)
            .unwrap_or(50);
        let participants: Vec<(PlayerId, i64)> = self
            .state
            .players
            .values()
            .filter(|p| p.life > 0)
            .map(|p| (p.player, p.level))
            .collect();
        let awards = self
            .xp_pipeline
            .distribute(base_xp, &participants, &self.parties);
        for (player, xp) in awards {
            if let Some(p) = self.state.players.get_mut(&player) {
                p.experience = p.experience.saturating_add(xp);
            }
        }
    }

    /// LootResolution phase (SPEC.md section 71): roll queued drops onto the
    /// ground. The drop seed derives from the game seed, monster id and
    /// death tick: fully reproducible.
    fn resolve_loot(&mut self) {
        let drops = std::mem::take(&mut self.pending_drops);
        for (monster, pos, drop_seed, level, tc) in drops {
            let item_level = 1 + level % 50;
            let rolled = self.loot_roller.roll(&tc, drop_seed, item_level);
            let tick = self.state.tick;
            let death_index = self.traces.death_index() + 1;
            let (tc_name, entry_name) =
                (format!("{tc:?}"), format!("{} entries", tc.entries.len()));
            match rolled {
                Ok(Some(item)) => {
                    let id = item.id;
                    let quality = format!("{:?}", item.quality);
                    let affix_count = item.affixes.len();
                    let level_id = self
                        .level
                        .as_ref()
                        .map(|l| l.id)
                        .unwrap_or(arpg_core::LevelInstanceId(0));
                    let born = self.state.tick.0;
                    self.inventory.spawn_ground(item, level_id, pos);
                    self.ground_spawn_ticks.insert(id, born);
                    let key = self.next_event_key(monster);
                    self.event_buffer.emit(key, GameEvent::ItemDropped(id));
                    self.traces.record_loot(crate::trace::LootTrace {
                        tick,
                        death_index,
                        monster,
                        drop_seed,
                        treasure_class: tc_name,
                        entry: entry_name,
                        base: format!("def {}", id.0),
                        quality,
                        affix_count,
                        item: Some(id),
                    });
                }
                Ok(None) => {
                    self.traces.record_loot(crate::trace::LootTrace {
                        tick,
                        death_index,
                        monster,
                        drop_seed,
                        treasure_class: tc_name,
                        entry: entry_name,
                        base: "none".into(),
                        quality: "none".into(),
                        affix_count: 0,
                        item: None,
                    });
                }
                Err(_) => {}
            }
        }
    }

    /// Expiration phase (SPEC.md section 88): ground items past their
    /// category lifetime vanish. None means no expiry during the game.
    pub const DEFAULT_GROUND_LIFETIME_TICKS: u64 = 5 * 25 * 60;

    fn resolve_expiration(&mut self) {
        // initial ruleset: a single ground lifetime for all categories
        let lifetime = Self::DEFAULT_GROUND_LIFETIME_TICKS;
        let expired: Vec<arpg_core::ItemId> = self
            .ground_spawn_ticks
            .iter()
            .filter(|(id, born)| {
                self.state.tick.0.saturating_sub(**born) > lifetime
                    && matches!(
                        self.inventory.location(**id),
                        Some(crate::item::ItemLocation::Ground(..))
                    )
            })
            .map(|(id, _)| *id)
            .collect();
        for id in expired {
            self.ground_spawn_ticks.remove(&id);
            self.inventory.remove(id);
            self.state.expired_items += 1;
        }
    }

    /// Last accepted command sequence for a player: the ack that lets a
    /// retransmitting client stop (SPEC section 186).
    /// Build the client replication for one player and surface the resync
    /// invariant (SPEC.md sections 145, 191): a client whose base is more
    /// than one revision behind demands a full resync.
    pub fn replication_for(&mut self, client: PlayerId) -> crate::replication::ClientReplication {
        let players = self.state.players.clone();
        let repl = crate::replication::build_client_replication(
            &mut self.replication,
            client,
            self.state.tick,
            &players,
        );
        if !repl.resync_entities.is_empty() {
            if let Some(m) = &self.metrics {
                m.lock()
                    .unwrap()
                    .incr(arpg_metrics::names::RESYNC_REQUESTED);
            }
        }
        repl
    }

    pub fn scheduler_last_accepted(&self, player: PlayerId) -> Option<u32> {
        self.scheduler.last_accepted_sequence(player)
    }

    /// Queue a quest event raised by gameplay; evaluated during
    /// QuestResolution (SPEC.md sections 8, 22).
    pub fn queue_quest_event(&mut self, trigger: crate::quest::QuestTrigger, source: EntityId) {
        let owner = self
            .state
            .players
            .keys()
            .find(|p| EntityId(p.0 as u64) == source)
            .copied();
        let eligible: Vec<PlayerId> = self.state.players.keys().copied().collect();
        self.pending_quest_events.push(crate::quest::QuestEvent {
            trigger,
            owner,
            eligible,
        });
    }

    /// QuestResolution phase (SPEC.md section 22): evaluate queued events,
    /// apply outcomes, clear the per-tick guard.
    fn resolve_quests(&mut self) {
        let events = std::mem::take(&mut self.pending_quest_events);
        for event in events {
            if let Ok(outcome) = self.quests.evaluate(&event) {
                for (player, wp) in outcome.waypoints_unlocked {
                    self.waypoints.unlock(
                        player,
                        self.quests
                            .active_difficulty
                            .unwrap_or(crate::quest::DifficultyId(0)),
                        wp,
                    );
                }
                for (player, area) in outcome.areas_unlocked {
                    let _ = (player, area);
                }
                for (owner, dest) in outcome.portals {
                    self.portals.open(crate::quest::Portal {
                        owner,
                        source: crate::quest::AreaId(0),
                        destination: dest,
                        access: crate::quest::QuestAccess::Party,
                        created_tick: self.state.tick.0,
                        expires_tick: Some(self.state.tick.0 + 30 * 25 * 60),
                    });
                }
            }
        }
        self.quests.end_tick();
    }

    /// Client pickup: validated and applied during InventoryTransactions
    /// (SPEC.md sections 85-86). The first valid pickup wins.
    pub fn pick_up_item(
        &mut self,
        player: PlayerId,
        item: arpg_core::ItemId,
        ground: crate::item::ItemLocation,
        to: crate::item::ItemLocation,
    ) -> Result<(), crate::item::ItemError> {
        if let Err(e) = self.inventory.pick_up(player, item, ground, to) {
            // invariant visibility (SPEC.md sections 191, 190): a pickup
            // against a vanished or stale location is observable
            if let Some(m) = &self.metrics {
                let mut m = m.lock().unwrap();
                m.incr(arpg_metrics::names::ITEM_TRANSACTION_FAILURES);
                if matches!(e, crate::item::ItemError::ItemUnavailable) {
                    m.incr(arpg_metrics::names::INVALID_ITEM_LOCATION);
                }
            }
            return Err(e);
        }
        self.ground_spawn_ticks.remove(&item);
        // Set bonuses are recomputed after equipment transactions
        // (SPEC.md section 89).
        if matches!(to, crate::item::ItemLocation::Equipment(_, _)) {
            self.recompute_set_bonuses(player);
        }
        let key = self.next_event_key(EntityId(player.0 as u64));
        self.event_buffer.emit(key, GameEvent::ItemPickedUp(item));
        Ok(())
    }

    /// Recompute the set bonuses of one player from their equipped
    /// items (SPEC.md section 89).
    pub fn recompute_set_bonuses(&mut self, player: PlayerId) -> i32 {
        let mut equipped = std::collections::BTreeMap::new();
        for (id, loc) in self.inventory.iter_locations() {
            if let crate::item::ItemLocation::Equipment(p, slot) = loc {
                if *p == player {
                    if let Some(def) = self.inventory.get(id).map(|i| i.definition) {
                        equipped.insert(*slot, def);
                    }
                }
            }
        }
        self.sets.recompute(player, &equipped)
    }

    /// Mark an entity as PendingDeath (SPEC.md section 13) with kill credit.
    pub fn apply_damage(&mut self, target: EntityId, source: EntityId, amount: i64) {
        if self.monsters.contains_key(&target) {
            self.apply_monster_damage(target, source, amount);
            return;
        }
        let Some(player_id) = self
            .state
            .players
            .keys()
            .find(|p| EntityId(p.0 as u64) == target)
            .copied()
        else {
            return;
        };
        // PvP damage gating (SPEC.md sections 113-114): a player can only
        // damage another player when the ruleset PvpMode allows it and the
        // pair is hostile. Party/Neutral pairs never damage each other.
        if let Some(attacker) = self
            .state
            .players
            .keys()
            .find(|p| EntityId(p.0 as u64) == source)
            .copied()
        {
            let relation = self.parties.relation(attacker, player_id, &self.hostility);
            if !self.hostility.damage_allowed(self.rules.pvp_mode, relation) {
                return;
            }
        }
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

/// Deterministic drop seed for a monster death: BLAKE3(game seed || entity ||
/// tick), matching the loot pipeline domain separation (SPEC.md section 71).
pub(crate) fn derive_drop_seed(seed: [u8; 32], entity: EntityId, tick: Tick) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&seed);
    hasher.update(&entity.0.to_le_bytes());
    hasher.update(&tick.0.to_le_bytes());
    *hasher.finalize().as_bytes()
}

/// Translate a datapack skill into the sim's typed skill IR. Returns None
/// for nominal skills without a program.
fn translate_skill(
    data_skill: &arpg_data::SkillDefinition,
) -> Option<crate::skill::SkillDefinition> {
    let program = data_skill.program.as_ref()?;
    let targeting = match program.targeting {
        0 => crate::skill::TargetingSpec::SelfTarget,
        1 => crate::skill::TargetingSpec::Entity,
        _ => crate::skill::TargetingSpec::Position,
    };
    let range = crate::damage::DamageRange::new(program.damage, program.damage);
    let damage = crate::damage::DamagePacket {
        physical: if program.damage_type == 0 {
            range
        } else {
            crate::damage::DamageRange::new(0, 0)
        },
        magic: if program.damage_type == 1 {
            range
        } else {
            crate::damage::DamageRange::new(0, 0)
        },
        fire: if program.damage_type == 2 {
            range
        } else {
            crate::damage::DamageRange::new(0, 0)
        },
        cold: if program.damage_type == 3 {
            range
        } else {
            crate::damage::DamageRange::new(0, 0)
        },
        lightning: if program.damage_type == 4 {
            range
        } else {
            crate::damage::DamageRange::new(0, 0)
        },
        poison: if program.damage_type == 5 {
            Some(crate::damage::PoisonPayload {
                total_damage_fp: program.damage * 256,
                remaining_ticks: 10,
                accumulator: 0,
            })
        } else {
            None
        },
    };
    let mut ops = vec![crate::skill::SkillOp::DealDamage(damage)];
    if let Some(missile) = program.missile {
        ops.push(crate::skill::SkillOp::SpawnMissile(missile));
    }
    Some(crate::skill::SkillDefinition {
        id: data_skill.id,
        targeting,
        cost: crate::skill::CostFormula {
            mana_cost: program.mana_cost,
            life_cost: 0,
        },
        timing: if program.cast_ticks == 0 {
            crate::skill::TimingFormula::Instant
        } else {
            crate::skill::TimingFormula::Ticks(program.cast_ticks)
        },
        program: crate::skill::SkillProgram { ops },
    })
}
