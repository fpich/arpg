pub mod brain;
pub mod packs;

use arpg_core::{EntityId, Tick, WorldPos};

/// AI perception snapshot passed to the think step (SPEC.md section 60).
/// The AI only observes; it never mutates the world directly.
#[derive(Debug, Clone, Default)]
pub struct PerceptionView {
    pub enemies: Vec<PerceivedEntity>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PerceivedEntity {
    pub entity: EntityId,
    pub pos: WorldPos,
}

/// AI blackboard (SPEC.md section 61): all per-agent working memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AiBlackboard {
    pub current_target: Option<EntityId>,
    pub last_known_target_pos: Option<WorldPos>,
    pub last_damage_source: Option<EntityId>,
    pub home_position: WorldPos,
    pub state_entered_tick: Tick,
}

/// HFSM states (SPEC.md section 59). Hierarchical: a child state may fall
/// back to its parent's behavior when it has no transition of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiState {
    Idle,
    Patrol,
    Aggro,
    /// Child of Aggro: move toward the target.
    Chase,
    /// Child of Aggro: attack in range.
    Attack,
    /// Child of Aggro: retreat to home position.
    Leash,
    Flee,
}

impl AiState {
    pub fn parent(self) -> Option<AiState> {
        match self {
            AiState::Chase | AiState::Attack | AiState::Leash => Some(AiState::Aggro),
            _ => None,
        }
    }

    /// Distance at which an aggro state transitions, in world units.
    pub fn is_aggro_child(self) -> bool {
        matches!(self, AiState::Chase | AiState::Attack | AiState::Leash)
    }
}

/// An AI-produced intention (SPEC.md section 60). The AI pipeline ends with
/// intent generation; the sim applies intents like client commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiIntent {
    MoveTo(WorldPos),
    AttackTarget(EntityId),
    None,
}

/// Per-definition AI parameters (SPEC.md sections 62-63).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AiParams {
    pub think_interval_ticks: u32,
    pub aggro_radius: i64,
    pub leash_radius: i64,
    pub attack_range: i64,
}

impl Default for AiParams {
    fn default() -> AiParams {
        AiParams {
            think_interval_ticks: 5,
            aggro_radius: 8 * 256,
            leash_radius: 40 * 256,
            attack_range: 2 * 256,
        }
    }
}

/// One AI agent: blackboard plus its current HFSM state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AiAgent {
    pub entity: EntityId,
    pub blackboard: AiBlackboard,
    pub state: AiState,
}

impl AiAgent {
    pub fn new(entity: EntityId, home: WorldPos) -> AiAgent {
        AiAgent {
            entity,
            blackboard: AiBlackboard {
                home_position: home,
                ..AiBlackboard::default()
            },
            state: AiState::Idle,
        }
    }

    /// Staggered think scheduling (SPEC.md section 62):
    /// (entity_id % think_interval) dephases the whole pack.
    pub fn should_think(&self, tick: Tick, think_interval: u32) -> bool {
        think_interval > 0
            && tick.0 % think_interval as u64 == self.entity.0 % think_interval as u64
    }

    /// Perception -> blackboard -> transition -> intent (SPEC.md section 60).
    pub fn think(
        &mut self,
        tick: Tick,
        current_pos: WorldPos,
        view: &PerceptionView,
        params: &AiParams,
    ) -> AiIntent {
        self.update_blackboard(current_pos, view, params);
        self.transition(tick, current_pos, view, params);
        self.generate_intent(params)
    }

    fn update_blackboard(
        &mut self,
        current_pos: WorldPos,
        view: &PerceptionView,
        params: &AiParams,
    ) {
        // Target selection (SPEC.md section 63): score, sort descending,
        // EntityId tie-break, no RNG unless the definition asks for it.
        if let Some(target) = select_target(self.entity, current_pos, view, params) {
            self.blackboard.current_target = Some(target.entity);
            self.blackboard.last_known_target_pos = Some(target.pos);
        }
    }

    fn transition(
        &mut self,
        tick: Tick,
        current_pos: WorldPos,
        view: &PerceptionView,
        params: &AiParams,
    ) {
        let home = self.blackboard.home_position;
        let previous = self.state;
        let target_pos = self.blackboard.last_known_target_pos;

        let new_state = match self.state {
            AiState::Idle | AiState::Patrol => {
                if view
                    .enemies
                    .iter()
                    .any(|e| e.pos.dist2(current_pos) <= params.aggro_radius * params.aggro_radius)
                {
                    AiState::Chase
                } else {
                    AiState::Idle
                }
            }
            AiState::Chase => match target_pos {
                Some(pos) => {
                    let target_near = view
                        .enemies
                        .iter()
                        .find(|e| Some(e.entity) == self.blackboard.current_target)
                        .is_some_and(|e| {
                            e.pos.dist2(current_pos) <= params.attack_range * params.attack_range
                        });
                    if home.dist2(pos) > params.leash_radius * params.leash_radius {
                        AiState::Leash
                    } else if target_near {
                        AiState::Attack
                    } else {
                        AiState::Chase
                    }
                }
                None => AiState::Idle,
            },
            AiState::Attack => {
                if target_pos.is_none() {
                    AiState::Idle
                } else {
                    AiState::Attack
                }
            }
            AiState::Leash => {
                let nearest = view
                    .enemies
                    .iter()
                    .map(|e| e.pos)
                    .min_by_key(|p| p.dist2(current_pos))
                    .unwrap_or(home);
                if current_pos.dist2(nearest) <= params.aggro_radius * params.aggro_radius {
                    AiState::Idle
                } else {
                    AiState::Leash
                }
            }
            AiState::Aggro | AiState::Flee => self.state,
        };
        if new_state != previous {
            self.state = new_state;
            self.blackboard.state_entered_tick = tick;
        }
    }

    fn generate_intent(&self, _params: &AiParams) -> AiIntent {
        match self.state {
            AiState::Idle => AiIntent::None,
            AiState::Patrol => AiIntent::MoveTo(self.blackboard.home_position),
            AiState::Chase => self
                .blackboard
                .last_known_target_pos
                .map(AiIntent::MoveTo)
                .unwrap_or(AiIntent::None),
            AiState::Attack => self
                .blackboard
                .current_target
                .map(AiIntent::AttackTarget)
                .unwrap_or(AiIntent::None),
            AiState::Leash => AiIntent::MoveTo(self.blackboard.home_position),
            AiState::Flee => AiIntent::MoveTo(self.blackboard.home_position),
            AiState::Aggro => AiIntent::None,
        }
    }
}

/// Deterministic target selection (SPEC.md section 63): nearest enemy wins;
/// EntityId breaks ties. No RNG.
pub fn select_target(
    self_entity: EntityId,
    self_pos: WorldPos,
    view: &PerceptionView,
    _params: &AiParams,
) -> Option<PerceivedEntity> {
    view.enemies
        .iter()
        .filter(|e| e.entity != self_entity)
        .min_by(|a, b| {
            a.pos
                .dist2(self_pos)
                .cmp(&b.pos.dist2(self_pos))
                .then(a.entity.cmp(&b.entity))
        })
        .copied()
}

pub use brain::HfsmBrain;
pub use packs::{ChampionModifier, ChampionProfile, FormationPolicy, MonsterPack, PackRole};
