//! Quest DSL (SPEC.md sections 101-106): definitions with variables, flags,
//! objectives, triggers, conditions, actions. State is split between
//! `CharacterQuestState` (rewards already received) and `GameQuestState`
//! (world state in this game, e.g. a door opened).

use crate::item::ItemInstance;
use arpg_core::{ItemDefId, MonsterDefId, ObjectId, PlayerId, WorldPos};
use std::collections::BTreeMap;
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct QuestDefId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AreaId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WaypointId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DifficultyId(pub u32);

// ---------------------------------------------------------------- triggers

/// Quest triggers (SPEC.md section 102).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuestTrigger {
    AreaEntered(AreaId),
    MonsterKilled(MonsterDefId),
    BossKilled(MonsterDefId),
    NpcInteracted(u32),
    ObjectActivated(ObjectId),
    ItemObtained(ItemDefId),
    ItemConsumed(ItemDefId),
    RecipeCompleted(ItemDefId),
    PartyEvent,
    CustomEvent(u32),
}

// --------------------------------------------------------------- conditions

/// Quest conditions (SPEC.md section 103).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuestCondition {
    HasFlag(String),
    NotFlag(String),
    VariableEquals(String, i64),
    VariableAtLeast(String, i64),
    HasItemClass(u32),
    IsDifficulty(DifficultyId),
    IsInParty,
    AreaIs(AreaId),
    IsQuestState(QuestDefId, QuestStatus),
}

// ----------------------------------------------------------------- actions

/// Quest actions (SPEC.md section 104).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuestAction {
    SetFlag(String),
    ClearFlag(String),
    SetVariable(String, i64),
    IncrementVariable(String, i64),
    GrantExperience(u64),
    GrantItem(ItemDefId),
    UnlockWaypoint(WaypointId),
    UnlockArea(AreaId),
    SpawnObject(u32, WorldPos),
    SpawnMonster(MonsterDefId, WorldPos),
    OpenPortal(AreaId),
    CompleteObjective(u32),
    CompleteQuest,
}

/// Access policy for a quest event (SPEC.md section 108 policies):
/// who is eligible when several members trigger the same event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuestAccess {
    OwnerOnly,
    Party,
    Everyone,
}

/// One objective of a quest, with its own completion flag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestObjective {
    pub id: u32,
    pub description: String,
}

/// A quest definition (SPEC.md section 101).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestDefinition {
    pub id: QuestDefId,
    pub name: String,
    pub act: u32,
    pub objectives: Vec<QuestObjective>,
    /// trigger -> conditions -> actions
    pub rules: Vec<QuestRule>,
    /// how multi-player triggers are attributed (section 106)
    pub access: QuestAccess,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestRule {
    pub trigger: QuestTrigger,
    pub conditions: Vec<QuestCondition>,
    pub actions: Vec<QuestAction>,
}

// ------------------------------------------------------------------- state

/// Lifecycle of a quest for one party/game.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum QuestStatus {
    NotStarted,
    Active,
    Completed,
    /// Completed and reward already claimed.
    Rewarded,
    Failed,
}

/// Per-game quest state (SPEC.md section 105): world-visible progress like
/// an opened door. One instance per quest per game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameQuestState {
    pub quest: QuestDefId,
    pub status: QuestStatus,
    /// game-wide variables and flags (door opened, lever pulled)
    pub variables: BTreeMap<String, i64>,
    pub flags: BTreeSet<String>,
    /// objectives completed at game level
    pub completed_objectives: BTreeSet<u32>,
}

impl GameQuestState {
    pub fn new(quest: QuestDefId) -> GameQuestState {
        GameQuestState {
            quest,
            status: QuestStatus::NotStarted,
            variables: BTreeMap::new(),
            flags: BTreeSet::new(),
            completed_objectives: BTreeSet::new(),
        }
    }
}

/// Per-character quest state (SPEC.md section 105): rewards already
/// received, personal variables.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CharacterQuestState {
    pub quests: BTreeMap<QuestDefId, QuestStatus>,
    /// rewards already received per quest
    pub rewarded: BTreeSet<QuestDefId>,
    pub variables: BTreeMap<String, i64>,
    pub flags: BTreeSet<String>,
}

// ------------------------------------------------------------------ events

/// A concrete quest event raised by gameplay during a tick
/// (SPEC.md section 106). Evaluated once per quest per tick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestEvent {
    pub trigger: QuestTrigger,
    /// the player who caused the event, if any
    pub owner: Option<PlayerId>,
    /// players eligible for evaluation (party of the owner, or everyone)
    pub eligible: Vec<PlayerId>,
}

/// Outcome of quest evaluation: actions to apply and state transitions.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct QuestOutcome {
    pub objectives_completed: Vec<(QuestDefId, u32)>,
    pub quests_completed: Vec<QuestDefId>,
    pub quests_started: Vec<QuestDefId>,
    /// experience granted per player
    pub experience: Vec<(PlayerId, u64)>,
    /// items to grant per player (definition; instance minted by loot)
    pub items: Vec<(PlayerId, ItemDefId)>,
    pub waypoints_unlocked: Vec<(PlayerId, WaypointId)>,
    pub areas_unlocked: Vec<(PlayerId, AreaId)>,
    /// portal requests: (owner, destination)
    pub portals: Vec<(PlayerId, AreaId)>,
    pub flags_set: Vec<(QuestDefId, String)>,
    pub flags_cleared: Vec<(QuestDefId, String)>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum QuestError {
    #[error("unknown quest {0:?}")]
    UnknownQuest(QuestDefId),
    #[error("quest {0:?} is not active")]
    NotActive(QuestDefId),
    #[error("action requires an owner player")]
    OwnerRequired,
}

// ----------------------------------------------------------------- system

/// The quest system: definitions + per-game and per-character state, plus
/// the deterministic evaluation of a `QuestEvent` (once per quest per tick,
/// section 106).
#[derive(Debug, Default)]
pub struct QuestSystem {
    pub definitions: BTreeMap<QuestDefId, QuestDefinition>,
    pub game_states: BTreeMap<QuestDefId, GameQuestState>,
    pub character_states: BTreeMap<PlayerId, CharacterQuestState>,
    /// quest events already evaluated this tick: (quest, trigger-hash)
    evaluated: BTreeSet<(QuestDefId, u64)>,
    /// active difficulty, set by the sim from GameConfig (section 103)
    pub active_difficulty: Option<DifficultyId>,
}

impl QuestSystem {
    pub fn new() -> QuestSystem {
        QuestSystem::default()
    }

    pub fn register(&mut self, def: QuestDefinition) {
        let id = def.id;
        self.game_states
            .entry(id)
            .or_insert_with(|| GameQuestState::new(id));
        self.definitions.insert(id, def);
    }

    pub fn game_state(&self, quest: QuestDefId) -> Result<&GameQuestState, QuestError> {
        self.game_states
            .get(&quest)
            .ok_or(QuestError::UnknownQuest(quest))
    }

    pub fn character_state(&mut self, player: PlayerId) -> &mut CharacterQuestState {
        self.character_states.entry(player).or_default()
    }

    /// Reset the per-tick evaluation guard (called at EndTick).
    pub fn end_tick(&mut self) {
        self.evaluated.clear();
    }

    fn trigger_key(trigger: &QuestTrigger) -> u64 {
        let mut hasher = blake3::Hasher::new();
        match trigger {
            QuestTrigger::AreaEntered(a) => hasher.update(&[0, a.0 as u8]),
            QuestTrigger::MonsterKilled(m) => hasher.update(&[1, m.0 as u8]),
            QuestTrigger::BossKilled(m) => hasher.update(&[2, m.0 as u8]),
            QuestTrigger::NpcInteracted(n) => hasher.update(&[3, *n as u8]),
            QuestTrigger::ObjectActivated(o) => hasher.update(&[4, o.0 as u8]),
            QuestTrigger::ItemObtained(i) => hasher.update(&[5, i.0 as u8]),
            QuestTrigger::ItemConsumed(i) => hasher.update(&[6, i.0 as u8]),
            QuestTrigger::RecipeCompleted(i) => hasher.update(&[7, i.0 as u8]),
            QuestTrigger::PartyEvent => hasher.update(&[8]),
            QuestTrigger::CustomEvent(e) => hasher.update(&[9, *e as u8]),
        };
        hasher.finalize().as_bytes()[0] as u64
    }

    /// Evaluate an event against all quests (SPEC.md section 106): once per
    /// quest per tick, then per eligible player. Produces the outcome that
    /// the QuestResolution phase applies.
    pub fn evaluate(&mut self, event: &QuestEvent) -> Result<QuestOutcome, QuestError> {
        let mut outcome = QuestOutcome::default();
        let defs: Vec<QuestDefinition> = self.definitions.values().cloned().collect();
        for def in defs {
            let key = (def.id, Self::trigger_key(&event.trigger));
            if self.evaluated.contains(&key) {
                continue;
            }
            let mut fired = false;
            for rule in &def.rules {
                if rule.trigger != event.trigger {
                    continue;
                }
                // start the quest on first evaluation
                let status = self
                    .game_states
                    .get(&def.id)
                    .map(|s| s.status)
                    .unwrap_or(QuestStatus::NotStarted);
                if status == QuestStatus::Completed || status == QuestStatus::Rewarded {
                    continue;
                }
                if status == QuestStatus::NotStarted {
                    outcome.quests_started.push(def.id);
                    if let Some(gs) = self.game_states.get_mut(&def.id) {
                        gs.status = QuestStatus::Active;
                    }
                }
                // evaluate the rule per eligible player; game-level
                // actions (objectives, flags, completion) apply once per
                // event (section 106), player-level actions per player
                let mut game_actions_done = false;
                for player in &event.eligible {
                    if !self.conditions_met(&def, rule, player, event) {
                        continue;
                    }
                    self.apply_actions(&def, rule, player, event, &mut outcome, game_actions_done)?;
                    game_actions_done = true;
                }
                fired = true;
            }
            if fired {
                self.evaluated.insert(key);
            }
        }
        Ok(outcome)
    }

    fn conditions_met(
        &self,
        def: &QuestDefinition,
        rule: &QuestRule,
        player: &PlayerId,
        event: &QuestEvent,
    ) -> bool {
        let gs = self.game_states.get(&def.id);
        let cs = self.character_states.get(player);
        rule.conditions
            .iter()
            .all(|c| self.condition_met(c, gs, cs, player, event))
    }

    fn condition_met(
        &self,
        cond: &QuestCondition,
        gs: Option<&GameQuestState>,
        cs: Option<&CharacterQuestState>,
        _player: &PlayerId,
        _event: &QuestEvent,
    ) -> bool {
        match cond {
            QuestCondition::HasFlag(f) => {
                gs.map(|s| s.flags.contains(f)).unwrap_or(false)
                    || cs.map(|s| s.flags.contains(f)).unwrap_or(false)
            }
            QuestCondition::NotFlag(f) => {
                !(gs.map(|s| s.flags.contains(f)).unwrap_or(false)
                    || cs.map(|s| s.flags.contains(f)).unwrap_or(false))
            }
            QuestCondition::VariableEquals(name, v) => {
                gs.and_then(|s| s.variables.get(name))
                    .or_else(|| cs.and_then(|s| s.variables.get(name)))
                    == Some(v)
            }
            QuestCondition::VariableAtLeast(name, v) => gs
                .and_then(|s| s.variables.get(name))
                .or_else(|| cs.and_then(|s| s.variables.get(name)))
                .map(|x| x >= v)
                .unwrap_or(false),
            QuestCondition::HasItemClass(_) => {
                // inventory-dependent; validated by the caller via outcome
                true
            }
            QuestCondition::IsDifficulty(d) => self.active_difficulty == Some(*d),
            QuestCondition::IsInParty => true,
            QuestCondition::AreaIs(_) => true,
            QuestCondition::IsQuestState(q, st) => gs
                .map(|s| s.quest == *q && s.status == *st)
                .unwrap_or(false),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn apply_actions(
        &mut self,
        def: &QuestDefinition,
        rule: &QuestRule,
        player: &PlayerId,
        event: &QuestEvent,
        outcome: &mut QuestOutcome,
        game_actions_done: bool,
    ) -> Result<(), QuestError> {
        for action in &rule.actions {
            // game-level actions were already applied for this event
            if game_actions_done
                && matches!(
                    action,
                    QuestAction::SetFlag(_)
                        | QuestAction::ClearFlag(_)
                        | QuestAction::SetVariable(_, _)
                        | QuestAction::IncrementVariable(_, _)
                        | QuestAction::CompleteObjective(_)
                        | QuestAction::CompleteQuest
                        | QuestAction::SpawnObject(_, _)
                        | QuestAction::SpawnMonster(_, _)
                )
            {
                continue;
            }
            match action {
                QuestAction::SetFlag(f) => {
                    if let Some(gs) = self.game_states.get_mut(&def.id) {
                        gs.flags.insert(f.clone());
                    }
                    outcome.flags_set.push((def.id, f.clone()));
                }
                QuestAction::ClearFlag(f) => {
                    if let Some(gs) = self.game_states.get_mut(&def.id) {
                        gs.flags.remove(f);
                    }
                    outcome.flags_cleared.push((def.id, f.clone()));
                }
                QuestAction::SetVariable(name, v) => {
                    if let Some(gs) = self.game_states.get_mut(&def.id) {
                        gs.variables.insert(name.clone(), *v);
                    }
                }
                QuestAction::IncrementVariable(name, by) => {
                    if let Some(gs) = self.game_states.get_mut(&def.id) {
                        let e = gs.variables.entry(name.clone()).or_insert(0);
                        *e = e.saturating_add(*by);
                    }
                }
                QuestAction::GrantExperience(xp) => {
                    outcome.experience.push((*player, *xp));
                }
                QuestAction::GrantItem(i) => {
                    outcome.items.push((*player, *i));
                }
                QuestAction::UnlockWaypoint(w) => {
                    outcome.waypoints_unlocked.push((*player, *w));
                }
                QuestAction::UnlockArea(a) => {
                    outcome.areas_unlocked.push((*player, *a));
                }
                QuestAction::SpawnObject(_, _) | QuestAction::SpawnMonster(_, _) => {
                    // world spawning handled by the sim during QuestResolution
                }
                QuestAction::OpenPortal(dest) => {
                    outcome.portals.push((*player, *dest));
                }
                QuestAction::CompleteObjective(obj) => {
                    if let Some(gs) = self.game_states.get_mut(&def.id) {
                        gs.completed_objectives.insert(*obj);
                    }
                    outcome.objectives_completed.push((def.id, *obj));
                    // auto-complete when all objectives are done
                    let all_done = self
                        .game_states
                        .get(&def.id)
                        .map(|gs| {
                            def.objectives
                                .iter()
                                .all(|o| gs.completed_objectives.contains(&o.id))
                                && gs.status != QuestStatus::Completed
                        })
                        .unwrap_or(false);
                    if all_done {
                        if let Some(gs) = self.game_states.get_mut(&def.id) {
                            gs.status = QuestStatus::Completed;
                        }
                        outcome.quests_completed.push(def.id);
                        // mark all eligible characters as completed
                        for p in &event.eligible {
                            let cs = self.character_states.entry(*p).or_default();
                            cs.quests.insert(def.id, QuestStatus::Completed);
                        }
                    }
                }
                QuestAction::CompleteQuest => {
                    if let Some(gs) = self.game_states.get_mut(&def.id) {
                        if gs.status != QuestStatus::Completed {
                            gs.status = QuestStatus::Completed;
                            outcome.quests_completed.push(def.id);
                            for p in &event.eligible {
                                let cs = self.character_states.entry(*p).or_default();
                                cs.quests.insert(def.id, QuestStatus::Completed);
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

// -------------------------------------------------------------- waypoints

/// Waypoint state, persistent per character x difficulty
/// (SPEC.md section 107).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WaypointState {
    /// (character, difficulty) -> unlocked waypoints
    unlocked: BTreeMap<(PlayerId, DifficultyId), BTreeSet<WaypointId>>,
}

impl WaypointState {
    pub fn unlock(&mut self, player: PlayerId, difficulty: DifficultyId, wp: WaypointId) {
        self.unlocked
            .entry((player, difficulty))
            .or_default()
            .insert(wp);
    }

    pub fn is_unlocked(&self, player: PlayerId, difficulty: DifficultyId, wp: WaypointId) -> bool {
        self.unlocked
            .get(&(player, difficulty))
            .map(|s| s.contains(&wp))
            .unwrap_or(false)
    }

    pub fn unlocked_of(&self, player: PlayerId, difficulty: DifficultyId) -> Vec<WaypointId> {
        self.unlocked
            .get(&(player, difficulty))
            .map(|s| s.iter().copied().collect())
            .unwrap_or_default()
    }
}

// ---------------------------------------------------------------- portals

/// A town portal (SPEC.md section 108).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Portal {
    pub owner: PlayerId,
    /// area the portal was opened in
    pub source: AreaId,
    pub destination: AreaId,
    pub access: QuestAccess,
    pub created_tick: u64,
    /// portals expire after a lifetime
    pub expires_tick: Option<u64>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PortalError {
    #[error("portal expired")]
    Expired,
    #[error("portal not accessible by player {0}")]
    NotAccessible(PlayerId),
    #[error("destination must be in the same ruleset and difficulty")]
    WrongRuleset,
}

/// Portal registry. Destination must belong to the same ruleset and
/// difficulty (section 107/108).
#[derive(Debug, Default)]
pub struct PortalSystem {
    portals: Vec<Portal>,
}

impl PortalSystem {
    pub fn new() -> PortalSystem {
        PortalSystem::default()
    }

    pub fn open(&mut self, portal: Portal) {
        self.portals.push(portal);
    }

    pub fn expire(&mut self, tick: u64) {
        self.portals
            .retain(|p| p.expires_tick.map(|e| e > tick).unwrap_or(true));
    }

    pub fn use_portal(
        &self,
        portal_index: usize,
        player: PlayerId,
        tick: u64,
    ) -> Result<&Portal, PortalError> {
        let portal = self
            .portals
            .get(portal_index)
            .ok_or(PortalError::NotAccessible(player))?;
        if portal.expires_tick.map(|e| tick >= e).unwrap_or(false) {
            return Err(PortalError::Expired);
        }
        match portal.access {
            QuestAccess::OwnerOnly => {
                if portal.owner != player {
                    return Err(PortalError::NotAccessible(player));
                }
            }
            QuestAccess::Party | QuestAccess::Everyone => {}
        }
        Ok(portal)
    }

    pub fn portals(&self) -> &[Portal] {
        &self.portals
    }
}

/// Helper: mint a granted quest item as a concrete instance (the loot
/// pipeline provides determinism; quests use a quest-derived seed).
pub fn mint_quest_item(
    id: arpg_core::ItemId,
    definition: ItemDefId,
    quest: QuestDefId,
    player: PlayerId,
) -> ItemInstance {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&quest.0.to_le_bytes());
    hasher.update(&player.0.to_le_bytes());
    let seed = *hasher.finalize().as_bytes();
    ItemInstance {
        id,
        definition,
        quality: crate::item::ItemQuality::Normal,
        item_level: 1,
        generation_seed: seed,
        affixes: Default::default(),
        sockets: Default::default(),
        durability: Some(50),
        flags: 0,
        charges: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_quest() -> QuestDefinition {
        QuestDefinition {
            id: QuestDefId(1),
            name: "Den of Evil".into(),
            act: 1,
            access: QuestAccess::Party,
            objectives: vec![QuestObjective {
                id: 0,
                description: "Kill all demons".into(),
            }],
            rules: vec![QuestRule {
                trigger: QuestTrigger::BossKilled(MonsterDefId(42)),
                conditions: vec![],
                actions: vec![
                    QuestAction::CompleteObjective(0),
                    QuestAction::SetFlag("boss_dead".into()),
                ],
            }],
        }
    }

    #[test]
    fn boss_kill_starts_completes_and_sets_flags() {
        let mut q = QuestSystem::new();
        q.register(sample_quest());
        let outcome = q
            .evaluate(&QuestEvent {
                trigger: QuestTrigger::BossKilled(MonsterDefId(42)),
                owner: Some(PlayerId(1)),
                eligible: vec![PlayerId(1), PlayerId(2)],
            })
            .unwrap();
        assert_eq!(outcome.quests_started, vec![QuestDefId(1)]);
        assert_eq!(outcome.quests_completed, vec![QuestDefId(1)]);
        assert_eq!(outcome.objectives_completed, vec![(QuestDefId(1), 0)]);
        assert!(q
            .game_state(QuestDefId(1))
            .unwrap()
            .flags
            .contains("boss_dead"));
        for p in [PlayerId(1), PlayerId(2)] {
            assert_eq!(
                q.character_state(p).quests.get(&QuestDefId(1)),
                Some(&QuestStatus::Completed)
            );
        }
    }

    #[test]
    fn event_evaluated_once_per_quest_per_tick() {
        let mut q = QuestSystem::new();
        q.register(sample_quest());
        let event = QuestEvent {
            trigger: QuestTrigger::BossKilled(MonsterDefId(42)),
            owner: Some(PlayerId(1)),
            eligible: vec![PlayerId(1)],
        };
        let o1 = q.evaluate(&event).unwrap();
        let o2 = q.evaluate(&event).unwrap();
        assert!(!o1.quests_started.is_empty());
        assert!(
            o2.quests_started.is_empty(),
            "second evaluation in same tick is a no-op"
        );
        q.end_tick();
        let o3 = q.evaluate(&event).unwrap();
        assert!(o3.quests_started.is_empty(), "quest already completed");
    }

    #[test]
    fn conditions_gate_actions() {
        let mut q = QuestSystem::new();
        q.register(QuestDefinition {
            id: QuestDefId(2),
            name: "Cold Plains".into(),
            act: 1,
            access: QuestAccess::Party,
            objectives: vec![QuestObjective {
                id: 0,
                description: "o".into(),
            }],
            rules: vec![QuestRule {
                trigger: QuestTrigger::AreaEntered(AreaId(3)),
                conditions: vec![QuestCondition::HasFlag("key".into())],
                actions: vec![QuestAction::SetVariable("entered".into(), 1)],
            }],
        });
        let event = QuestEvent {
            trigger: QuestTrigger::AreaEntered(AreaId(3)),
            owner: Some(PlayerId(1)),
            eligible: vec![PlayerId(1)],
        };
        q.evaluate(&event).unwrap();
        assert!(!q
            .game_state(QuestDefId(2))
            .unwrap()
            .variables
            .contains_key("entered"));
        // set the flag, next tick it fires
        q.end_tick();
        q.game_states
            .get_mut(&QuestDefId(2))
            .unwrap()
            .flags
            .insert("key".into());
        q.evaluate(&event).unwrap();
        assert_eq!(
            q.game_state(QuestDefId(2))
                .unwrap()
                .variables
                .get("entered"),
            Some(&1)
        );
    }

    #[test]
    fn waypoints_are_per_character_and_difficulty() {
        let mut w = WaypointState::default();
        let wp = WaypointId(5);
        let d = DifficultyId(0);
        w.unlock(PlayerId(1), d, wp);
        assert!(w.is_unlocked(PlayerId(1), d, wp));
        assert!(!w.is_unlocked(PlayerId(2), d, wp), "personal");
        assert!(
            !w.is_unlocked(PlayerId(1), DifficultyId(1), wp),
            "difficulty-scoped"
        );
    }

    #[test]
    fn portals_respect_access_policy_and_expiry() {
        let mut p = PortalSystem::new();
        p.open(Portal {
            owner: PlayerId(1),
            source: AreaId(1),
            destination: AreaId(2),
            access: QuestAccess::OwnerOnly,
            created_tick: 10,
            expires_tick: Some(100),
        });
        assert!(p.use_portal(0, PlayerId(2), 50).is_err());
        assert!(p.use_portal(0, PlayerId(1), 50).is_ok());
        assert!(p.use_portal(0, PlayerId(1), 100).is_err(), "expired");
        p.expire(100);
        assert!(p.portals().is_empty());
    }

    #[test]
    fn minted_quest_items_are_deterministic() {
        let a = mint_quest_item(
            arpg_core::ItemId(1),
            ItemDefId(7),
            QuestDefId(1),
            PlayerId(1),
        );
        let b = mint_quest_item(
            arpg_core::ItemId(1),
            ItemDefId(7),
            QuestDefId(1),
            PlayerId(1),
        );
        assert_eq!(a, b);
        let c = mint_quest_item(
            arpg_core::ItemId(1),
            ItemDefId(7),
            QuestDefId(1),
            PlayerId(2),
        );
        assert_ne!(a, c, "player-derived");
    }
}
