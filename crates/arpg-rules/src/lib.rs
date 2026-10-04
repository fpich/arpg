#[derive(Debug, Clone)]
pub struct GameRules {
    pub max_players: u8,
    pub difficulty: u32,
    pub pvp_mode: PvpMode,
    pub ruleset_hash: [u8; 32],
}

impl Default for GameRules {
    fn default() -> GameRules {
        GameRules {
            max_players: 8,
            difficulty: 0,
            pvp_mode: PvpMode::Hostility,
            ruleset_hash: [0; 32],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LootMode {
    FreeForAll,
    RoundRobin,
    Instanced,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PvpMode {
    Disabled,
    Consent,
    Hostility,
    Arena,
}

/// Chance to hit (SPEC.md section 47, ruleset D2-like):
/// CTH = 2×AR / (AR+Defense) × attackerLvl / (attackerLvl+defenderLvl),
/// clamped to 5%..95%. This formula belongs to the RULESET, not ENGINE.
pub fn chance_to_hit(
    attack_rating: i64,
    defense: i64,
    attacker_level: i64,
    defender_level: i64,
) -> i64 {
    let ar = attack_rating.max(1);
    let def = defense.max(1);
    let al = attacker_level.max(1);
    let dl = defender_level.max(1);
    let cth = 2 * ar * 100 / (ar + def) * al / (al + dl);
    cth.clamp(5, 95)
}

/// Player-count scaling curve (SPEC.md section 111): an explicit list of
/// (player_count, multiplier_bp) breakpoints. No implicit "+6% per player"
/// exists in the engine - the ruleset chooses its values.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ScalingCurve {
    /// (player count, multiplier in basis points; 10_000 = base).
    /// Interpolation is stepped: the last breakpoint <= player count wins;
    /// (0, 10_000) is the implicit floor.
    pub breakpoints: Vec<(u32, i64)>,
}

impl ScalingCurve {
    /// A flat curve: every player count scales to base (10_000 bp).
    pub fn base() -> ScalingCurve {
        ScalingCurve {
            breakpoints: vec![(0, 10_000)],
        }
    }

    /// Multiplier in basis points for a given connected player count.
    pub fn multiplier_bp(&self, player_count: u32) -> i64 {
        let mut best: i64 = 10_000;
        for &(count, bp) in &self.breakpoints {
            if player_count >= count {
                best = bp;
            }
        }
        best
    }
}

/// Monster scaling by player count (SPEC.md section 111): exposed by the
/// engine, chosen by the ruleset. Applied at spawn only (section 112) -
/// a player joining or leaving never rescales an existing monster.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonsterScalingRules {
    pub health: ScalingCurve,
    pub damage: ScalingCurve,
    pub accuracy: ScalingCurve,
    pub density: ScalingCurve,
    pub elite_frequency: ScalingCurve,
    pub experience: ScalingCurve,
    pub loot: ScalingCurve,
}

impl Default for MonsterScalingRules {
    fn default() -> MonsterScalingRules {
        // Default ruleset: flat, no scaling per player (the D2-like
        // ruleset overrides with its own chosen values).
        MonsterScalingRules {
            health: ScalingCurve::base(),
            damage: ScalingCurve::base(),
            accuracy: ScalingCurve::base(),
            density: ScalingCurve::base(),
            elite_frequency: ScalingCurve::base(),
            experience: ScalingCurve::base(),
            loot: ScalingCurve::base(),
        }
    }
}

/// PvP scaling (SPEC.md section 115): dedicated multipliers, separate
/// from monster scaling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PvpRules {
    /// Damage multiplier between players, basis points.
    pub damage_bp: i64,
    /// Whether friendly-fire needs declared hostility.
    pub requires_hostility: bool,
}

impl Default for PvpRules {
    fn default() -> PvpRules {
        PvpRules {
            damage_bp: 10_000,
            requires_hostility: true,
        }
    }
}

#[cfg(test)]
mod scaling_tests {
    use super::*;

    #[test]
    fn curves_step_by_player_count() {
        let curve = ScalingCurve {
            breakpoints: vec![(0, 10_000), (2, 12_000), (4, 15_000)],
        };
        assert_eq!(curve.multiplier_bp(1), 10_000);
        assert_eq!(curve.multiplier_bp(2), 12_000);
        assert_eq!(curve.multiplier_bp(3), 12_000);
        assert_eq!(curve.multiplier_bp(8), 15_000);
    }

    #[test]
    fn flat_default_never_scales() {
        let rules = MonsterScalingRules::default();
        for players in 1..=8 {
            assert_eq!(rules.health.multiplier_bp(players), 10_000);
            assert_eq!(rules.damage.multiplier_bp(players), 10_000);
        }
    }

    #[test]
    fn pvp_rules_default_full_damage_with_hostility() {
        let pvp = PvpRules::default();
        assert_eq!(pvp.damage_bp, 10_000);
        assert!(pvp.requires_hostility);
    }
}
