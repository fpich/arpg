use crate::command::UseSkillIntent;

/// Targeting specification of a skill (SPEC.md section 35).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetingSpec {
    SelfTarget,
    Entity,
    Position,
    Direction,
    NoTarget,
}

/// Skill cost (SPEC.md section 35). Costs are non-negative unless the
/// datapack explicitly allows negatives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CostFormula {
    pub mana_cost: i64,
    pub life_cost: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimingFormula {
    Instant,
    Ticks(u16),
    /// Attack-scaled timing (SPEC.md section 56): the base ticks
    /// shrink through the AttackSpeed curve instead of CastSpeed, so
    /// melee swing rate benefits from attack-speed bonuses.
    AttackTicks(u16),
}

/// Typed skill intermediate representation (SPEC.md section 36). The IR is
/// not Turing-complete: no unbounded loops, fixed opcodes only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillOp {
    Sequence(Vec<SkillOp>),
    DealDamage(crate::damage::DamagePacket),
    Heal(i64),
    RestoreMana(i64),
    ApplyState(u32, u32),
    RemoveState(u32),
    SpawnMissile(u32),
    Summon(u32),
    Knockback(i32),
    ModifyStat(arpg_core::StatId, crate::stat::ModifierOp, i64),
    Teleport,
    /// Corpse explosion (SPEC.md sections 36, 100): consumes the nearest
    /// unconsumed corpse to the target and adds its damage to the
    /// outcome; without a corpse the op contributes nothing.
    ConsumeCorpse(i64),
    /// Native effect (SPEC.md section 38): effects unreasonable to express
    /// in the IR. Every native id must have a documented justification, a
    /// dedicated unit test, deterministic input/output, no I/O and no
    /// implicit RNG. Target: >= 90% of skills without any native effect.
    Native(NativeEffectId),
    NoOp,
}

/// Registered native effects (SPEC.md section 38).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum NativeEffectId {
    /// Chain lightning arc (sections 36, 38): the number of chained
    /// targets and their order depend on live entity geometry around the
    /// impact point - unreasonable to express as a static IR program.
    /// Justification: dynamic multi-target selection. Deterministic:
    /// engine enumerates candidates in canonical entity-id order and the
    /// callback folds them into the outcome. No I/O, no RNG.
    ChainLightning { max_targets: u8 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillProgram {
    pub ops: Vec<SkillOp>,
}

/// A full skill definition (SPEC.md section 35).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillDefinition {
    pub id: arpg_core::SkillId,
    pub targeting: TargetingSpec,
    pub cost: CostFormula,
    pub timing: TimingFormula,
    pub program: SkillProgram,
}

/// Validation error kinds (SPEC.md section 37).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SkillValidationError(pub &'static str);

/// Maximum sequence nesting depth: recursion and nesting are bounded.
pub const MAX_SKILL_DEPTH: usize = 16;
/// Maximum ops per sequence: selections have a maximum size.
pub const MAX_SEQUENCE_OPS: usize = 64;

impl SkillDefinition {
    /// Validate at load time (SPEC.md section 37). An invalid skill must
    /// prevent datapack loading.
    pub fn validate(&self) -> Result<(), SkillValidationError> {
        if self.cost.mana_cost < 0 || self.cost.life_cost < 0 {
            return Err(SkillValidationError("negative cost"));
        }
        let mut op_count = 0usize;
        self.validate_op(&self.program.ops, 0, &mut op_count)?;
        Ok(())
    }

    fn validate_op(
        &self,
        ops: &[SkillOp],
        depth: usize,
        op_count: &mut usize,
    ) -> Result<(), SkillValidationError> {
        if depth > MAX_SKILL_DEPTH {
            return Err(SkillValidationError("nesting too deep"));
        }
        if ops.len() > MAX_SEQUENCE_OPS {
            return Err(SkillValidationError("sequence too large"));
        }
        for op in ops {
            *op_count += 1;
            if *op_count > MAX_SEQUENCE_OPS * MAX_SKILL_DEPTH {
                return Err(SkillValidationError("unbounded program"));
            }
            if let SkillOp::Sequence(inner) = op {
                self.validate_op(inner, depth + 1, op_count)?;
            }
        }
        Ok(())
    }
}

/// Execution outcome of a skill program against a single target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SkillOutcome {
    pub damage: i64,
    pub heal: i64,
    pub mana_restored: i64,
    pub states_applied: u32,
    pub knockback: i32,
}

/// Execute a skill program in a pure, deterministic way (SPEC.md section 36).
/// Missile and summon ops record their intent via the `spawn` callback so the
/// engine stays in charge of entity creation.
pub fn execute_program(
    program: &SkillProgram,
    intent: &UseSkillIntent,
    mut spawn_missile: impl FnMut(u32),
    mut consume_corpse: impl FnMut(i64) -> bool,
    mut native: impl FnMut(NativeEffectId, &mut SkillOutcome),
) -> SkillOutcome {
    let mut outcome = SkillOutcome::default();
    execute_ops(
        &program.ops,
        intent,
        &mut outcome,
        &mut spawn_missile,
        &mut consume_corpse,
        &mut native,
    );
    outcome
}

fn execute_ops(
    ops: &[SkillOp],
    intent: &UseSkillIntent,
    outcome: &mut SkillOutcome,
    spawn_missile: &mut impl FnMut(u32),
    consume_corpse: &mut impl FnMut(i64) -> bool,
    native: &mut impl FnMut(NativeEffectId, &mut SkillOutcome),
) {
    for op in ops {
        match op {
            SkillOp::Sequence(inner) => execute_ops(
                inner,
                intent,
                outcome,
                spawn_missile,
                consume_corpse,
                native,
            ),
            SkillOp::DealDamage(packet) => {
                outcome.damage = outcome.damage.saturating_add(sum_packet_mid(packet));
            }
            SkillOp::Heal(amount) => outcome.heal = outcome.heal.saturating_add(*amount),
            SkillOp::RestoreMana(amount) => {
                outcome.mana_restored = outcome.mana_restored.saturating_add(*amount)
            }
            SkillOp::ApplyState(_, _) => outcome.states_applied += 1,
            SkillOp::RemoveState(_) => {}
            SkillOp::SpawnMissile(def) => spawn_missile(*def),
            SkillOp::Summon(_) => {}
            SkillOp::Knockback(dist) => outcome.knockback = outcome.knockback.saturating_add(*dist),
            SkillOp::ModifyStat(_, _, _) => {}
            SkillOp::Teleport => {
                let _ = intent.target;
            }
            SkillOp::ConsumeCorpse(damage) => {
                // the engine callback decides whether a corpse is
                // available near the target position
                if consume_corpse(*damage) {
                    outcome.damage = outcome.damage.saturating_add(*damage);
                }
            }
            SkillOp::Native(effect) => {
                // section 38: the engine resolves the native effect and
                // folds its contribution into the outcome deterministically
                native(*effect, outcome);
            }
            SkillOp::NoOp => {}
        }
    }
}

fn sum_packet_mid(packet: &crate::damage::DamagePacket) -> i64 {
    let mid = |r: &crate::damage::DamageRange| (r.min + r.max) / 2;
    mid(&packet.physical)
        + mid(&packet.magic)
        + mid(&packet.fire)
        + mid(&packet.cold)
        + mid(&packet.lightning)
}
