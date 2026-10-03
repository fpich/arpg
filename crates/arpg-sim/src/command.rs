use arpg_core::{PlayerId, SkillId, Tick, WorldPos};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MovementMode {
    Walk,
    Run,
    Forced,
    Knockback,
    Teleport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MoveIntent {
    pub direction: WorldPos,
    pub movement_mode: MovementMode,
    pub sequence: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientCommand {
    Move(MoveIntent),
    UseSkill {
        skill: SkillId,
        target: Option<WorldPos>,
    },
    NoOp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandEnvelope {
    pub sequence: u32,
    pub client_tick: Tick,
    pub player: PlayerId,
    pub command: ClientCommand,
}
