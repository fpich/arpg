//! Secondary damage effects (SPEC.md section 50): critical strike,
//! deadly strike, crushing blow, open wounds, knockback, life/mana
//! leech, reflect/thorns and prevent-healing. All rolls are drawn from
//! a dedicated combat domain so the outcome stays deterministic.

/// Secondary-effect profile of an attacker (SPEC.md section 50), in
/// basis points. Zero means the effect never triggers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SecondaryProfile {
    /// Critical strike: doubles the damage of the hit.
    pub critical_strike_bp: i32,
    /// Deadly strike: doubles the physical damage (stacks with crit).
    pub deadly_strike_bp: i32,
    /// Crushing blow: percentage-of-life damage, reduced against
    /// non-normal targets by `crushing_factor_divisor`.
    pub crushing_blow_bp: i32,
    /// Open wounds: bleed over time applied as a poison-like DoT.
    pub open_wounds_bp: i32,
    /// Knockback: pushes the target back on hit.
    pub knockback_bp: i32,
    /// Life leech: percentage of final damage returned as life.
    pub life_leech_bp: i32,
    /// Mana leech: percentage of final damage returned as mana.
    pub mana_leech_bp: i32,
    /// Reflect/thorns: percentage of final damage returned to the
    /// attacker.
    pub thorns_bp: i32,
    /// Prevent healing: blocks healing on the target for a duration.
    pub prevent_healing_bp: i32,
}

/// The secondary outcome of one hit (SPEC.md section 50).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SecondaryOutcome {
    pub critical: bool,
    pub deadly: bool,
    pub crushing: bool,
    pub open_wounds: bool,
    pub knockback: bool,
    /// Percentage-of-life damage from crushing blow, in fixed-point
    /// whole units to subtract.
    pub crushing_amount: i64,
    /// Life returned to the attacker.
    pub life_leech: i64,
    /// Mana returned to the attacker.
    pub mana_leech: i64,
    /// Damage reflected to the attacker.
    pub thorns: i64,
    pub prevent_healing: bool,
}

/// Deterministic roll: BLAKE3(seed || attacker || defender || tick ||
/// attack counter || domain tag), one draw per effect in canonical
/// order, so the outcome only depends on the game seed and the inputs.
pub fn resolve(
    profile: &SecondaryProfile,
    roll: impl Fn(u8) -> u64,
    final_damage: i64,
    target_current_life: i64,
    target_max_life: i64,
) -> SecondaryOutcome {
    let bp_hit = |bp: i32, draw: u64| bp > 0 && (draw % 10_000) < bp as u64;
    let critical = bp_hit(profile.critical_strike_bp, roll(0));
    let deadly = bp_hit(profile.deadly_strike_bp, roll(1));
    let crushing = bp_hit(profile.crushing_blow_bp, roll(2));
    let open_wounds = bp_hit(profile.open_wounds_bp, roll(3));
    let knockback = bp_hit(profile.knockback_bp, roll(4));
    let prevent_healing = bp_hit(profile.prevent_healing_bp, roll(5));
    // Crushing blow: percentage of the target's current life (SPEC.md
    // section 50), a fixed 10% baseline scaled by the profile.
    let crushing_amount = if crushing {
        let bp = profile.crushing_blow_bp.min(10_000) as i64;
        // percentage of current life; against bosses the effect is a
        // tenth (handled by the caller passing an already-scaled bp)
        (target_current_life.saturating_mul(bp) / 10_000).min(target_max_life / 8)
    } else {
        0
    };
    let leech_base = final_damage.max(0);
    let life_leech = (leech_base * profile.life_leech_bp as i64) / 10_000;
    let mana_leech = (leech_base * profile.mana_leech_bp as i64) / 10_000;
    let thorns = (leech_base * profile.thorns_bp as i64) / 10_000;
    SecondaryOutcome {
        critical,
        deadly,
        crushing,
        open_wounds,
        knockback,
        crushing_amount,
        life_leech,
        mana_leech,
        thorns,
        prevent_healing,
    }
}

/// Apply critical/deadly multipliers to the rolled damage (SPEC.md
/// section 50): critical doubles everything, deadly doubles the whole
/// packet in this simplified model; both can stack to x4.
pub fn amplify(base: i64, outcome: &SecondaryOutcome) -> i64 {
    let mut total = base;
    if outcome.critical {
        total = total.saturating_mul(2);
    }
    if outcome.deadly {
        total = total.saturating_mul(2);
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixed_rolls(hit: u64) -> impl Fn(u8) -> u64 {
        move |_| hit
    }

    #[test]
    fn critical_doubles_damage() {
        let profile = SecondaryProfile {
            critical_strike_bp: 5000,
            ..Default::default()
        };
        let out = resolve(&profile, fixed_rolls(0), 100, 1000, 1000);
        assert!(out.critical);
        assert_eq!(amplify(100, &out), 200);
    }

    #[test]
    fn crit_and_deadly_stack_to_x4() {
        let profile = SecondaryProfile {
            critical_strike_bp: 5000,
            deadly_strike_bp: 5000,
            ..Default::default()
        };
        let out = resolve(&profile, fixed_rolls(0), 100, 1000, 1000);
        assert_eq!(amplify(100, &out), 400);
    }

    #[test]
    fn crushing_blow_is_percentage_of_life() {
        let profile = SecondaryProfile {
            crushing_blow_bp: 10_000,
            ..Default::default()
        };
        let out = resolve(&profile, fixed_rolls(0), 50, 1000, 1000);
        assert!(out.crushing);
        // 100% bp would take the whole life, but crushing is capped at
        // 1/8 of max life per hit
        assert_eq!(out.crushing_amount, 125);
    }

    #[test]
    fn leech_and_thorns_scale_with_damage() {
        let profile = SecondaryProfile {
            life_leech_bp: 1000,
            mana_leech_bp: 500,
            thorns_bp: 2500,
            ..Default::default()
        };
        let out = resolve(&profile, fixed_rolls(u64::MAX), 200, 1000, 1000);
        assert_eq!(out.life_leech, 20);
        assert_eq!(out.mana_leech, 10);
        assert_eq!(out.thorns, 50);
        assert!(!out.critical);
    }

    #[test]
    fn zero_profile_never_triggers() {
        let out = resolve(
            &SecondaryProfile::default(),
            fixed_rolls(0),
            100,
            1000,
            1000,
        );
        assert!(!out.critical && !out.deadly && !out.crushing);
        assert_eq!(out.crushing_amount, 0);
    }
}
