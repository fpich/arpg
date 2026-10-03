use arpg_core::{ActionPhase, ActorMode, EntityId, Tick, WorldPos};

/// Per-action timing in ticks (SPEC.md section 33).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ActionTiming {
    pub windup_ticks: u16,
    pub impact_tick: u16,
    pub recovery_ticks: u16,
}

/// Interruption priority (SPEC.md section 34): a lower-priority interruption
/// never replaces an already scheduled higher-priority one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum InterruptPriority {
    PlayerCancel,
    HitRecovery,
    Knockback,
    Stun,
    Death,
}

impl InterruptPriority {
    pub fn for_mode(mode: ActorMode) -> Option<InterruptPriority> {
        match mode {
            ActorMode::HitRecovery => Some(InterruptPriority::HitRecovery),
            ActorMode::Knockback => Some(InterruptPriority::Knockback),
            ActorMode::Stunned => Some(InterruptPriority::Stun),
            ActorMode::Dead => Some(InterruptPriority::Death),
            _ => None,
        }
    }
}

/// Target of an active action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Entity(EntityId),
    Position(WorldPos),
    None,
}

/// A single in-flight action (SPEC.md section 32). Each actor has at most
/// one ActionState; this is it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActiveAction {
    pub id: u64,
    /// Skill reference when the action is a Cast (SPEC.md section 32).
    pub skill: Option<arpg_core::SkillId>,
    pub mode: ActorMode,
    pub start_tick: Tick,
    pub phase: ActionPhase,
    pub target: Target,
    pub timing: ActionTiming,
    pub pending_interrupt: Option<InterruptPriority>,
}

impl ActiveAction {
    pub fn new(
        id: u64,
        mode: ActorMode,
        start_tick: Tick,
        target: Target,
        timing: ActionTiming,
    ) -> ActiveAction {
        ActiveAction {
            id,
            skill: None,
            mode,
            start_tick,
            phase: ActionPhase::Windup,
            target,
            timing,
            pending_interrupt: None,
        }
    }

    /// Advance the action's phase given the current tick (SPEC.md
    /// section 33). Returns true when the action completed this tick.
    pub fn advance(&mut self, tick: Tick) -> bool {
        let elapsed = tick.0.saturating_sub(self.start_tick.0);
        match self.phase {
            ActionPhase::Windup => {
                if elapsed >= self.timing.windup_ticks as u64 {
                    self.phase = ActionPhase::Impact;
                }
                false
            }
            ActionPhase::Impact => {
                self.phase = ActionPhase::Recovery;
                false
            }
            ActionPhase::Recovery => {
                if elapsed >= (self.timing.windup_ticks + self.timing.recovery_ticks) as u64 {
                    self.phase = ActionPhase::Complete;
                    true
                } else {
                    false
                }
            }
            ActionPhase::Complete => true,
        }
    }

    /// Try to schedule an interruption (SPEC.md section 34).
    pub fn request_interrupt(&mut self, priority: InterruptPriority) -> bool {
        match self.pending_interrupt {
            Some(existing) if existing >= priority => false,
            _ => {
                self.pending_interrupt = Some(priority);
                true
            }
        }
    }

    /// Apply the pending interruption and return the resulting mode when the
    /// action is cut short.
    pub fn apply_interrupt(&mut self) -> Option<ActorMode> {
        let priority = self.pending_interrupt?;
        self.pending_interrupt = None;
        self.phase = ActionPhase::Complete;
        Some(match priority {
            InterruptPriority::Death => ActorMode::Dead,
            InterruptPriority::Stun => ActorMode::Stunned,
            InterruptPriority::Knockback => ActorMode::Knockback,
            InterruptPriority::HitRecovery => ActorMode::HitRecovery,
            InterruptPriority::PlayerCancel => ActorMode::Neutral,
        })
    }
}

/// The actor itself: lifecycle, action state, stats (SPEC.md section 32).
#[derive(Debug, Clone)]
pub struct Actor {
    pub entity: EntityId,
    pub mode: ActorMode,
    pub lifecycle: arpg_core::Lifecycle,
    pub action: Option<ActiveAction>,
    pub stats: crate::stat::StatBlock,
    pub action_sequence: u64,
}

impl Actor {
    pub fn new(entity: EntityId) -> Actor {
        Actor {
            entity,
            mode: ActorMode::Neutral,
            lifecycle: arpg_core::Lifecycle::Alive,
            action: None,
            stats: crate::stat::StatBlock::new(),
            action_sequence: 0,
        }
    }

    /// Start a new action; a pending death actor cannot act (SPEC.md
    /// section 13).
    pub fn start_action(
        &mut self,
        mode: ActorMode,
        tick: Tick,
        target: Target,
        timing: ActionTiming,
    ) -> Option<u64> {
        if self.lifecycle != arpg_core::Lifecycle::Alive {
            return None;
        }
        self.action_sequence += 1;
        let id = self.action_sequence;
        self.action = Some(ActiveAction::new(id, mode, tick, target, timing));
        self.mode = mode;
        Some(id)
    }
}
