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
    /// XP pipeline configuration (SPEC.md section 110).
    pub xp_pipeline: crate::social::XpPipeline,
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
        self.next_monster_entity += 1;
        let entity = EntityId(self.next_monster_entity);
        self.monsters.insert(
            entity,
            MonsterState {
                entity,
                pos: home,
                life: 50,
                treasure_class,
            },
        );
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
                    _ => {}
                }
            }
        }

        if let Phase::Perception = phase {
            self.ai_perceive();
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
    fn monster_attack(&mut self, monster: EntityId, target: EntityId) {
        let Some(m) = self.monsters.get(&monster) else {
            return;
        };
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
        let amount = 5i64;
        self.apply_damage(target, monster, amount);
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
        const MONSTER_BASE_XP: u64 = 50;
        let participants: Vec<(PlayerId, i64)> = self
            .state
            .players
            .values()
            .filter(|p| p.life > 0)
            .map(|p| (p.player, p.level))
            .collect();
        let awards = self
            .xp_pipeline
            .distribute(MONSTER_BASE_XP, &participants, &self.parties);
        for (player, xp) in awards {
            if let Some(p) = self.state.players.get_mut(&player) {
                p.experience = p.experience.saturating_add(xp);
            }
        }
        let _ = monster;
    }

    /// LootResolution phase (SPEC.md section 71): roll queued drops onto the
    /// ground. The drop seed derives from the game seed, monster id and
    /// death tick: fully reproducible.
    fn resolve_loot(&mut self) {
        let drops = std::mem::take(&mut self.pending_drops);
        for (monster, pos, drop_seed, level, tc) in drops {
            let item_level = 1 + level % 50;
            if let Ok(Some(item)) = self.loot_roller.roll(&tc, drop_seed, item_level) {
                let id = item.id;
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
        self.inventory.pick_up(player, item, ground, to)?;
        self.ground_spawn_ticks.remove(&item);
        let key = self.next_event_key(EntityId(player.0 as u64));
        self.event_buffer.emit(key, GameEvent::ItemPickedUp(item));
        Ok(())
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
fn derive_drop_seed(seed: [u8; 32], entity: EntityId, tick: Tick) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&seed);
    hasher.update(&entity.0.to_le_bytes());
    hasher.update(&tick.0.to_le_bytes());
    *hasher.finalize().as_bytes()
}
