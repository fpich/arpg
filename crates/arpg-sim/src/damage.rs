/// Inclusive damage range in fixed-point units.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DamageRange {
    pub min: i64,
    pub max: i64,
}

impl DamageRange {
    pub fn new(min: i64, max: i64) -> DamageRange {
        DamageRange { min, max }
    }

    pub fn is_zero(&self) -> bool {
        self.min == 0 && self.max == 0
    }
}

/// Poison payload (SPEC.md section 53): a DoT keeps a fixed-point
/// accumulator; damage is applied over remaining ticks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PoisonPayload {
    pub total_damage_fp: i64,
    pub remaining_ticks: u32,
    pub accumulator: i64,
}

/// Damage packet (SPEC.md section 49). Each type may be zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DamagePacket {
    pub physical: DamageRange,
    pub magic: DamageRange,
    pub fire: DamageRange,
    pub cold: DamageRange,
    pub lightning: DamageRange,
    pub poison: Option<PoisonPayload>,
}

impl DamagePacket {
    pub fn is_zero(&self) -> bool {
        self.physical.is_zero()
            && self.magic.is_zero()
            && self.fire.is_zero()
            && self.cold.is_zero()
            && self.lightning.is_zero()
            && self.poison.is_none()
    }
}

/// Per-type resistances in basis points and their caps (SPEC.md section 51).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Resistances {
    pub physical_bp: i32,
    pub magic_bp: i32,
    pub fire_bp: i32,
    pub cold_bp: i32,
    pub lightning_bp: i32,
    pub poison_bp: i32,
}

/// Immunity: resistance >= 100% => immune (SPEC.md section 52).
fn is_immune(resist_bp: i32) -> bool {
    resist_bp >= 10_000
}

/// Apply a resistance in basis points to an amount. Negative resistance
/// amplifies damage. Result is clamped to >= 0.
pub fn apply_resist(amount: i64, resist_bp: i32) -> i64 {
    if is_immune(resist_bp) {
        return 0;
    }
    let scaled = amount.saturating_mul(10_000 - resist_bp as i64) / 10_000;
    scaled.max(0)
}

/// Resolve one damage packet against a defender's resistances (SPEC.md
/// sections 49-52): offensive modifiers are assumed already applied; this
/// applies resistance then sums the vital delta.
pub fn resolve_damage(roll: RollAmounts, resists: &Resistances) -> i64 {
    let physical = apply_resist(roll.physical, resists.physical_bp);
    let magic = apply_resist(roll.magic, resists.magic_bp);
    let fire = apply_resist(roll.fire, resists.fire_bp);
    let cold = apply_resist(roll.cold, resists.cold_bp);
    let lightning = apply_resist(roll.lightning, resists.lightning_bp);
    physical + magic + fire + cold + lightning
}

/// The rolled concrete amounts of one packet instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RollAmounts {
    pub physical: i64,
    pub magic: i64,
    pub fire: i64,
    pub cold: i64,
    pub lightning: i64,
}

impl RollAmounts {
    pub fn from_ranges(
        packet: &DamagePacket,
        mut roll_range: impl FnMut(&DamageRange) -> i64,
    ) -> RollAmounts {
        RollAmounts {
            physical: roll_range(&packet.physical),
            magic: roll_range(&packet.magic),
            fire: roll_range(&packet.fire),
            cold: roll_range(&packet.cold),
            lightning: roll_range(&packet.lightning),
        }
    }
}

/// Basis points helper: percentage of a value, integer division toward zero.
pub fn bp_of(value: i64, bp: i32) -> i64 {
    value.saturating_mul(bp as i64) / 10_000
}

/// Fixed-point conversion for DoT accumulators (SPEC.md section 53): the
/// per-tick share uses an accumulator so the total is exact regardless of
/// rounding.
pub struct DotAccumulator {
    pub total_damage_fp: i64,
    pub remaining_ticks: u32,
    pub accumulator: i64,
}

impl DotAccumulator {
    pub fn new(total_damage_fp: i64, ticks: u32) -> DotAccumulator {
        DotAccumulator {
            total_damage_fp,
            remaining_ticks: ticks,
            accumulator: 0,
        }
    }

    /// Amount of whole fixed-point units to apply this tick.
    pub fn tick_amount(&mut self) -> i64 {
        if self.remaining_ticks == 0 {
            return 0;
        }
        let share = self.total_damage_fp / self.remaining_ticks as i64;
        self.accumulator += share;
        self.remaining_ticks -= 1;
        if self.remaining_ticks == 0 {
            let final_payment = self.total_damage_fp - self.accumulator;
            self.accumulator += final_payment;
            share + final_payment
        } else {
            share
        }
    }
}

/// Convert fixed-point damage (1/256 units) to whole units, floor.
pub fn fixed_to_units(value_fp: i64) -> i64 {
    value_fp.div_euclid(256)
}
