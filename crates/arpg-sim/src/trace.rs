//! Combat and loot audit traces (SPEC.md sections 167-168). POLICY domain:
//! traces are observations of decisions already made; they never feed the
//! state hash and never alter gameplay. Every item must be auditable
//! (section 168) and every attack must explain its roll (section 167).

use arpg_core::{EntityId, ItemId, Tick};

/// One attack resolution, mirroring the section 167 format:
/// source, target, AR, Defense, CTH, roll, hit, damage breakdown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttackTrace {
    pub tick: Tick,
    pub attack_index: u64,
    pub source: EntityId,
    pub target: EntityId,
    pub attack_rating: i64,
    pub defense: i64,
    pub chance_bp: i64,
    pub roll_bp: i64,
    pub hit: bool,
    pub physical_raw: i64,
    pub resistance_percent: i64,
    pub final_damage: i64,
}

impl AttackTrace {
    /// Render in the section 167 debug text format.
    pub fn render(&self) -> String {
        format!(
            "Attack #{attack_index} (tick {tick})\nSource     {source}\nTarget     {target}\nHit:\n  AR          {ar}\n  Defense     {def}\n  Chance      {chance} bp\n  Roll        {roll} bp\n  Result      {result}\nDamage:\n  Physical raw  {raw}\n  Resistance     {res} %\n  Final          {final}",
            attack_index = self.attack_index,
            tick = self.tick.0,
            source = self.source.0,
            target = self.target.0,
            ar = self.attack_rating,
            def = self.defense,
            chance = self.chance_bp,
            roll = self.roll_bp,
            result = if self.hit { "Hit" } else { "Miss" },
            raw = self.physical_raw,
            res = self.resistance_percent,
            final = self.final_damage,
        )
    }
}

/// One loot generation, mirroring the section 168 format: death, seed,
/// treasure class chain, base, quality, affixes, item id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LootTrace {
    pub tick: Tick,
    pub death_index: u64,
    pub monster: EntityId,
    pub drop_seed: [u8; 32],
    pub treasure_class: String,
    pub entry: String,
    pub base: String,
    pub quality: String,
    pub affix_count: usize,
    pub item: Option<ItemId>,
}

impl LootTrace {
    /// Render in the section 168 debug text format.
    pub fn render(&self) -> String {
        let seed = self
            .drop_seed
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        format!(
            "DeathEvent {death} (tick {tick})\nMonster {monster}\nDropSeed {seed}\nTC {tc}\n\u{2192} entry {entry}\n\u{2192} base {base}\n\u{2192} quality {quality}\n\u{2192} affix {affix}\n\u{2192} ItemId {item}",
            death = self.death_index,
            tick = self.tick.0,
            monster = self.monster.0,
            seed = seed,
            tc = self.treasure_class,
            entry = self.entry,
            base = self.base,
            quality = self.quality,
            affix = self.affix_count,
            item = self.item.map(|i| i.0.to_string()).unwrap_or_else(|| "none".into()),
        )
    }
}

/// Ring buffer of recent traces. Bounded so long sessions cannot grow it
/// without limit (section 170: no unbounded allocation).
#[derive(Debug, Default)]
pub struct TraceBuffer {
    pub attacks: std::collections::VecDeque<AttackTrace>,
    pub loots: std::collections::VecDeque<LootTrace>,
    attack_index: u64,
    death_index: u64,
}

const MAX_TRACES: usize = 256;

impl TraceBuffer {
    pub fn new() -> TraceBuffer {
        TraceBuffer::default()
    }

    pub fn record_attack(&mut self, trace: AttackTrace) {
        if self.attacks.len() == MAX_TRACES {
            self.attacks.pop_front();
        }
        self.attack_index += 1;
        self.attacks.push_back(trace);
    }

    pub fn record_loot(&mut self, trace: LootTrace) {
        if self.loots.len() == MAX_TRACES {
            self.loots.pop_front();
        }
        self.death_index += 1;
        self.loots.push_back(trace);
    }

    pub fn attack_index(&self) -> u64 {
        self.attack_index
    }

    pub fn death_index(&self) -> u64 {
        self.death_index
    }

    pub fn recent_attacks(&self) -> impl Iterator<Item = &AttackTrace> {
        self.attacks.iter()
    }

    pub fn recent_loots(&self) -> impl Iterator<Item = &LootTrace> {
        self.loots.iter()
    }
}
