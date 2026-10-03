use arpg_core::{EntityId, Tick, WorldPos};

/// Read-only world view handed to the AI each Perception phase (SPEC.md
/// section 60): the AI observes, it never mutates the world.
#[derive(Debug, Clone, Default)]
pub struct AiWorldView {
    pub players: Vec<(EntityId, WorldPos)>,
    pub monsters: Vec<(EntityId, WorldPos)>,
}

/// A command produced by an AI brain. The sim applies them like client
/// intents; the brain itself stays pure (SPEC.md section 60).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiCommand {
    MoveTo(WorldPos),
    Attack(EntityId),
}

/// AI brain plugged into the AiDecision phase. Implemented by arpg-ai;
/// arpg-sim cannot depend on arpg-ai (crate graph, SPEC.md section 6), so
/// the engine only knows this trait.
pub trait AiBrain: Send {
    /// Called when the engine spawns a monster so the brain can track it.
    fn on_spawn(&mut self, _entity: EntityId, _home: WorldPos) {}

    /// Perception -> ... -> intent generation. Returns the commands to
    /// apply this tick, in canonical entity order.
    fn think(&mut self, tick: Tick, view: &AiWorldView) -> Vec<(EntityId, AiCommand)>;
}
