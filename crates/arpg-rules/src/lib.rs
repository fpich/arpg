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
