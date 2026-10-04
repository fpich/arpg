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

/// A derived stat (SPEC.md section 42): declares its dependencies so
/// the evaluation order is explicit and cycles are detectable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DerivedStat {
    pub stat: StatId,
    /// Stats this derivation reads; must be evaluated first.
    pub dependencies: Vec<StatId>,
    /// Contribution added per point of each dependency, aligned by
    /// index with `dependencies`.
    pub contribution_per_point: Vec<i64>,
    /// Flat addition applied after the dependency contributions.
    pub flat_add: i64,
}

impl DerivedStat {
    /// Evaluate the derived stat from resolved dependency values
    /// (SPEC.md section 42).
    pub fn evaluate(&self, dependency_values: &[i64]) -> i64 {
        let mut total = self.flat_add;
        for (i, per_point) in self.contribution_per_point.iter().enumerate() {
            if let Some(v) = dependency_values.get(i) {
                total = total.saturating_add(v.saturating_mul(*per_point));
            }
        }
        total
    }
}

/// A declared stat graph (SPEC.md section 42): derived stats with
/// explicit dependencies, evaluated in topological order with cycle
/// detection.
#[derive(Debug, Default, Clone)]
pub struct StatGraph {
    pub derived: BTreeMap<StatId, DerivedStat>,
}

impl StatGraph {
    pub fn new() -> StatGraph {
        StatGraph::default()
    }

    /// Evaluate one derived stat recursively (SPEC.md section 42):
    /// dependencies first, derived contributions second. Returns None
    /// on a dependency cycle.
    pub fn evaluate(&self, stat: StatId, block: &StatBlock) -> Option<i64> {
        let mut stack = Vec::new();
        self.eval_inner(stat, block, &mut stack)
    }

    fn eval_inner(&self, stat: StatId, block: &StatBlock, stack: &mut Vec<StatId>) -> Option<i64> {
        if stack.contains(&stat) {
            return None; // dependency cycle
        }
        match self.derived.get(&stat) {
            Some(def) => {
                stack.push(stat);
                let mut values = Vec::with_capacity(def.dependencies.len());
                for dep in def.dependencies.clone() {
                    values.push(self.eval_inner(dep, block, stack)?);
                }
                stack.pop();
                Some(def.evaluate(&values))
            }
            None => Some(block.compute(stat)),
        }
    }

    /// Validate the graph: every derived stat must be acyclic
    /// (SPEC.md section 42).
    pub fn validate(&self) -> Result<(), &'static str> {
        let stats: Vec<StatId> = self.derived.keys().copied().collect();
        for stat in stats {
            let block = StatBlock::new();
            if self.evaluate(stat, &block).is_none() {
                return Err("stat dependency cycle detected");
            }
        }
        Ok(())
    }
}

/// Canonical stat ids (SPEC.md sections 39-42): shared by equipment,
/// states and the derived-stat graph so modifiers compose in one block.
pub const STAT_STRENGTH: StatId = StatId(1);
pub const STAT_DEXTERITY: StatId = StatId(2);
pub const STAT_VITALITY: StatId = StatId(3);
pub const STAT_ENERGY: StatId = StatId(4);
pub const STAT_PHYSICAL_DAMAGE_BONUS: StatId = StatId(10);
pub const STAT_ATTACK_RATING: StatId = StatId(11);
pub const STAT_DEFENSE: StatId = StatId(12);

/// The reference derived-stat graph (SPEC.md section 42): strength
/// feeds physical damage, dexterity feeds attack rating and defense.
/// Acyclic by construction; validated at load time.
pub fn default_stat_graph() -> StatGraph {
    let mut graph = StatGraph::new();
    graph.derived.insert(
        STAT_PHYSICAL_DAMAGE_BONUS,
        DerivedStat {
            stat: STAT_PHYSICAL_DAMAGE_BONUS,
            dependencies: vec![STAT_STRENGTH],
            contribution_per_point: vec![1],
            flat_add: 0,
        },
    );
    graph.derived.insert(
        STAT_ATTACK_RATING,
        DerivedStat {
            stat: STAT_ATTACK_RATING,
            dependencies: vec![STAT_DEXTERITY],
            contribution_per_point: vec![5],
            flat_add: 0,
        },
    );
    graph.derived.insert(
        STAT_DEFENSE,
        DerivedStat {
            stat: STAT_DEFENSE,
            dependencies: vec![STAT_DEXTERITY],
            contribution_per_point: vec![2],
            flat_add: 0,
        },
    );
    graph
}

#[cfg(test)]
mod graph_tests {
    use super::*;
    use arpg_core::StatId;

    const STRENGTH: StatId = StatId(1);
    const DEXTERITY: StatId = StatId(2);
    const PHYSICAL_DAMAGE_BONUS: StatId = StatId(10);
    const ATTACK_RATING: StatId = StatId(11);
    const BLOCK_CHANCE: StatId = StatId(12);

    fn d2_graph() -> StatGraph {
        let mut graph = StatGraph::new();
        graph.derived.insert(
            PHYSICAL_DAMAGE_BONUS,
            DerivedStat {
                stat: PHYSICAL_DAMAGE_BONUS,
                dependencies: vec![STRENGTH],
                contribution_per_point: vec![1],
                flat_add: 0,
            },
        );
        graph.derived.insert(
            ATTACK_RATING,
            DerivedStat {
                stat: ATTACK_RATING,
                dependencies: vec![DEXTERITY],
                contribution_per_point: vec![5],
                flat_add: 0,
            },
        );
        graph.derived.insert(
            BLOCK_CHANCE,
            DerivedStat {
                stat: BLOCK_CHANCE,
                dependencies: vec![DEXTERITY],
                contribution_per_point: vec![2],
                flat_add: 100,
            },
        );
        graph
    }

    #[test]
    fn derived_stats_follow_declared_dependencies() {
        let graph = d2_graph();
        let mut block = StatBlock::new();
        block.set_base(STRENGTH, 30);
        block.set_base(DEXTERITY, 25);
        assert_eq!(graph.evaluate(PHYSICAL_DAMAGE_BONUS, &block), Some(30));
        assert_eq!(graph.evaluate(ATTACK_RATING, &block), Some(125));
        assert_eq!(graph.evaluate(BLOCK_CHANCE, &block), Some(150));
    }

    #[test]
    fn transitive_dependencies_resolve() {
        let mut graph = d2_graph();
        // total attack rating depends on the derived attack rating
        graph.derived.insert(
            StatId(20),
            DerivedStat {
                stat: StatId(20),
                dependencies: vec![ATTACK_RATING],
                contribution_per_point: vec![2],
                flat_add: 50,
            },
        );
        let mut block = StatBlock::new();
        block.set_base(DEXTERITY, 10);
        // 5*10 = 50 base AR, doubled = 100, plus 50 flat
        assert_eq!(graph.evaluate(StatId(20), &block), Some(150));
    }

    #[test]
    fn cycles_are_detected() {
        let mut graph = StatGraph::new();
        graph.derived.insert(
            StatId(1),
            DerivedStat {
                stat: StatId(1),
                dependencies: vec![StatId(2)],
                contribution_per_point: vec![1],
                flat_add: 0,
            },
        );
        graph.derived.insert(
            StatId(2),
            DerivedStat {
                stat: StatId(2),
                dependencies: vec![StatId(1)],
                contribution_per_point: vec![1],
                flat_add: 0,
            },
        );
        assert!(graph.evaluate(StatId(1), &StatBlock::new()).is_none());
        assert!(graph.validate().is_err());
    }

    #[test]
    fn acyclic_graph_validates() {
        assert!(d2_graph().validate().is_ok());
    }
}
