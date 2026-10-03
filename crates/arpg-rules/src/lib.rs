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
