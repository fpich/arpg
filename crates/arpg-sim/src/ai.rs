use arpg_core::{EntityId, Tick, WorldPos};

/// Read-only world view handed to the AI each Perception phase (SPEC.md
/// section 60): the AI observes, it never mutates the world.
#[derive(Debug, Clone, Default)]
pub struct AiWorldView {
    pub players: Vec<(EntityId, WorldPos)>,
    pub monsters: Vec<(EntityId, WorldPos)>,
    /// Per-monster perception stats from the datapack definition (SPEC.md
    /// sections 57, 60): aggro range in fixed points, ranged flag.
    pub monster_stats: Vec<(EntityId, MonsterPerception)>,
}

/// Datapack-backed perception stats for one monster.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonsterPerception {
    pub aggro_range_fp: i64,
    pub ranged: bool,
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

    /// Aggro alert (SPEC.md section 64): the engine reports that an
    /// entity was attacked; linked pack members should turn on the
    /// attacker even outside perception range.
    fn aggro_alert(&mut self, _entity: EntityId, _attacker: EntityId) {}
}
