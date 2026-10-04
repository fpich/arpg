//! Speed systems (SPEC.md section 56): AttackSpeed, CastSpeed, Block,
//! Recovery and HitRecovery are separate systems with distinct
//! parameters sharing a common diminishing-returns curve. None of them
//! depend on client animations.

/// Diminishing-returns curve shared by all speed systems (SPEC.md
/// section 56): each additional point of bonus contributes less. The
/// effective bonus is `total_bp * curve_denominator / (curve_denominator + total_bp)`
/// so the result approaches but never reaches the asymptote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiminishingCurve {
    /// Asymptote in basis points the effective bonus approaches.
    pub asymptote_bp: i64,
}

impl DiminishingCurve {
    /// Apply the curve to a raw bonus in basis points.
    pub fn apply(&self, raw_bp: i64) -> i64 {
        if raw_bp <= 0 {
            return raw_bp;
        }
        // effective = asymptote * raw / (asymptote + raw): classic
        // hyperbolic diminishing returns
        (self.asymptote_bp * raw_bp) / (self.asymptote_bp + raw_bp)
    }
}

/// The five separate speed systems (SPEC.md section 56). Each carries
/// its own curve parameters; they never share tunables implicitly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpeedSystems {
    /// Attack speed bonus curve.
    pub attack: DiminishingCurve,
    /// Cast speed bonus curve.
    pub cast: DiminishingCurve,
    /// Block chance curve.
    pub block: DiminishingCurve,
    /// Recovery (regen tick interval) curve.
    pub recovery: DiminishingCurve,
    /// Hit recovery (stun-break speed) curve.
    pub hit_recovery: DiminishingCurve,
}

impl Default for SpeedSystems {
    fn default() -> Self {
        SpeedSystems {
            attack: DiminishingCurve { asymptote_bp: 3000 },
            cast: DiminishingCurve { asymptote_bp: 3000 },
            block: DiminishingCurve { asymptote_bp: 7500 },
            recovery: DiminishingCurve { asymptote_bp: 2000 },
            hit_recovery: DiminishingCurve { asymptote_bp: 5000 },
        }
    }
}

/// Effective attack interval in ticks from a raw speed bonus (SPEC.md
/// section 56): the base interval shrinks by the curved bonus, with a
/// hard floor of 1 tick.
pub fn attack_interval_ticks(base_ticks: u16, raw_bonus_bp: i64, systems: &SpeedSystems) -> u16 {
    interval_ticks(base_ticks, systems.attack.apply(raw_bonus_bp))
}

/// Effective cast time in ticks (SPEC.md section 56).
pub fn cast_ticks(base_ticks: u16, raw_bonus_bp: i64, systems: &SpeedSystems) -> u16 {
    interval_ticks(base_ticks, systems.cast.apply(raw_bonus_bp))
}

/// Effective block chance in basis points (SPEC.md section 56): base
/// chance plus curved bonus, capped at the block asymptote.
pub fn block_chance_bp(base_bp: i32, raw_bonus_bp: i64, systems: &SpeedSystems) -> i32 {
    (base_bp as i64 + systems.block.apply(raw_bonus_bp)).clamp(0, 7500) as i32
}

/// Effective hit-recovery duration in ticks (SPEC.md section 56): the
/// base recovery shrinks with the curved bonus, floored at 1 tick.
pub fn hit_recovery_ticks(base_ticks: u16, raw_bonus_bp: i64, systems: &SpeedSystems) -> u16 {
    interval_ticks(base_ticks, systems.hit_recovery.apply(raw_bonus_bp))
}

/// Effective recovery interval in ticks (SPEC.md section 56).
pub fn recovery_interval_ticks(base_ticks: u16, raw_bonus_bp: i64, systems: &SpeedSystems) -> u16 {
    interval_ticks(base_ticks, systems.recovery.apply(raw_bonus_bp))
}

fn interval_ticks(base_ticks: u16, effective_bonus_bp: i64) -> u16 {
    if base_ticks == 0 {
        return 0;
    }
    let scaled = base_ticks as i64 * 10_000 / (10_000 + effective_bonus_bp.max(0));
    scaled.max(1) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    fn systems() -> SpeedSystems {
        SpeedSystems::default()
    }

    #[test]
    fn curve_diminishes() {
        let curve = DiminishingCurve { asymptote_bp: 3000 };
        let a = curve.apply(1000);
        let b = curve.apply(2000);
        // the second thousand adds less than the first
        assert!(b - a < a);
        // and the result never reaches the asymptote
        assert!(curve.apply(1_000_000) <= 3000);
    }

    #[test]
    fn negative_bonus_passes_through() {
        let curve = DiminishingCurve { asymptote_bp: 3000 };
        assert_eq!(curve.apply(-500), -500);
    }

    #[test]
    fn attack_speed_shrinks_interval_with_floor() {
        let sys = systems();
        assert_eq!(attack_interval_ticks(10, 0, &sys), 10);
        // the asymptote caps the effective bonus, so a huge raw bonus
        // converges to the asymptotic interval, never below
        let asymptotic = attack_interval_ticks(10, 10_000_000, &sys);
        let bounded = attack_interval_ticks(10, 10_000_000_000, &sys);
        assert_eq!(asymptotic, bounded);
        assert!(asymptotic < 10);
        // a tiny base reaches the 1-tick floor
        assert_eq!(attack_interval_ticks(2, 10_000_000, &sys), 1);
        // moderate bonus shrinks proportionally to the curved value
        let effective = sys.attack.apply(3000);
        let expected = (10 * 10_000 / (10_000 + effective)) as u16;
        assert_eq!(attack_interval_ticks(10, 3000, &sys), expected);
    }

    #[test]
    fn cast_and_attack_are_independent_systems() {
        // the systems remain separate parameters: distinct asymptotes
        // are representable
        let mut custom = systems();
        custom.cast.asymptote_bp = 1000;
        assert_eq!(cast_ticks(10, 3000, &custom), {
            let e = custom.cast.apply(3000);
            (10 * 10_000 / (10_000 + e)) as u16
        });
        assert_ne!(custom.cast.asymptote_bp, custom.attack.asymptote_bp);
    }

    #[test]
    fn block_chance_capped() {
        let sys = systems();
        assert_eq!(block_chance_bp(2500, 0, &sys), 2500);
        // the curve approaches the cap from below
        let huge = block_chance_bp(0, 100_000_000, &sys);
        assert!(huge <= 7500 && huge > 7400);
        assert_eq!(block_chance_bp(-100, 0, &sys), 0);
    }

    #[test]
    fn hit_recovery_floors_at_one_tick() {
        let sys = systems();
        assert_eq!(hit_recovery_ticks(6, 0, &sys), 6);
        // huge bonus converges below the base but stays above the floor
        let fast = hit_recovery_ticks(6, 10_000_000, &sys);
        assert!((1..6).contains(&fast));
        // a tiny base reaches the floor
        assert_eq!(hit_recovery_ticks(2, 10_000_000, &sys), 1);
    }
}
