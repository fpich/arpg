use arpg_core::StatId;
use std::collections::BTreeMap;

/// Stat modifier operations (SPEC.md section 41).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModifierOp {
    FlatAdd,
    PercentAddBp,
    MultiplyBp,
    Override,
    MinClamp,
    MaxClamp,
}

/// Stable ordering step of an operation in the normative pipeline.
impl ModifierOp {
    fn stage(self) -> u8 {
        match self {
            ModifierOp::FlatAdd => 0,
            ModifierOp::PercentAddBp => 1,
            ModifierOp::MultiplyBp => 2,
            ModifierOp::Override => 3,
            ModifierOp::MinClamp => 4,
            ModifierOp::MaxClamp => 5,
        }
    }
}

/// Source of a stat contribution (SPEC.md section 39). Values are never
/// stored as finals only: each contribution keeps its origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ModifierSource {
    Base,
    Equipment(u64),
    Skill(u32),
    State(u32),
    Aura(u64),
    Difficulty(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatModifier {
    pub stat: StatId,
    pub source: ModifierSource,
    pub source_sequence: u64,
    pub operation: ModifierOp,
    pub value: i64,
    pub priority: i16,
}

/// A stat block: base values plus ordered contributions (SPEC.md sections
/// 39-45). Same-stage modifiers are ordered by priority, then source, then
/// sequence; the result is independent of insertion order.
#[derive(Debug, Default, Clone)]
pub struct StatBlock {
    base: BTreeMap<StatId, i64>,
    modifiers: Vec<StatModifier>,
}

impl StatBlock {
    pub fn new() -> StatBlock {
        StatBlock::default()
    }

    pub fn set_base(&mut self, stat: StatId, value: i64) {
        self.base.insert(stat, value);
    }

    pub fn base_value(&self, stat: StatId) -> i64 {
        self.base.get(&stat).copied().unwrap_or(0)
    }

    pub fn add_modifier(&mut self, modifier: StatModifier) {
        self.modifiers.push(modifier);
    }

    pub fn clear_source(&mut self, source: ModifierSource) {
        self.modifiers.retain(|m| m.source != source);
    }

    /// Normative pipeline (SPEC.md section 40):
    /// Base -> FlatAdd -> PercentAddBp -> MultiplyBp -> Override ->
    /// MinClamp -> MaxClamp.
    pub fn compute(&self, stat: StatId) -> i64 {
        let mut value = self.base_value(stat);
        let mut relevant: Vec<&StatModifier> =
            self.modifiers.iter().filter(|m| m.stat == stat).collect();
        relevant.sort_by(|a, b| {
            a.operation
                .stage()
                .cmp(&b.operation.stage())
                .then(a.priority.cmp(&b.priority))
                .then(a.source.cmp(&b.source))
                .then(a.source_sequence.cmp(&b.source_sequence))
        });
        let mut override_applied = false;
        for m in relevant {
            match m.operation {
                ModifierOp::FlatAdd => value = value.saturating_add(m.value),
                ModifierOp::PercentAddBp => {
                    value = value.saturating_add(value.saturating_mul(m.value) / 10_000)
                }
                ModifierOp::MultiplyBp => value = value.saturating_mul(m.value) / 10_000,
                ModifierOp::Override => {
                    if !override_applied {
                        value = m.value;
                        override_applied = true;
                    } else if m.value > value {
                        value = m.value;
                    }
                }
                ModifierOp::MinClamp => value = value.max(m.value),
                ModifierOp::MaxClamp => value = value.min(m.value),
            }
        }
        value
    }
}
