use arpg_core::{EntityId, SkillId, Tick};
use std::collections::BTreeMap;

/// Stacking policy of a state (SPEC.md section 54).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StackPolicy {
    Refresh,
    Stack,
    Replace,
    Strongest,
    UniqueBySource,
}

/// A state applied to an entity (SPEC.md section 54).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateInstance {
    pub state: u32,
    pub source: EntityId,
    pub source_skill: Option<SkillId>,
    pub applied_tick: Tick,
    pub expires_tick: Option<Tick>,
    pub stack_key: (u32, u64),
    /// Strength of the state in basis points (SPEC.md section 54):
    /// resistance magnitude, slow percentage, etc. Zero for untyped states.
    pub magnitude_bp: i32,
}

impl StateInstance {
    pub fn is_expired(&self, tick: Tick) -> bool {
        self.expires_tick.is_some_and(|e| tick >= e)
    }
}

/// State store keyed by (entity, state, source) for deterministic iteration.
#[derive(Debug, Default, Clone)]
pub struct StateStore {
    entries: BTreeMap<(EntityId, u32, u64), StateInstance>,
}

impl StateStore {
    pub fn new() -> StateStore {
        StateStore::default()
    }

    pub fn apply(&mut self, entity: EntityId, instance: StateInstance, policy: StackPolicy) {
        let key = (entity, instance.state, instance.source.0);
        match policy {
            StackPolicy::Replace => {
                self.entries.insert(key, instance);
            }
            StackPolicy::Refresh => {
                if let Some(existing) = self.entries.get_mut(&key) {
                    existing.expires_tick = instance.expires_tick;
                    existing.applied_tick = instance.applied_tick;
                } else {
                    self.entries.insert(key, instance);
                }
            }
            StackPolicy::Stack => {
                self.entries.insert(key, instance);
            }
            StackPolicy::Strongest => {
                if let Some(existing) = self.entries.get(&key) {
                    if instance.expires_tick >= existing.expires_tick {
                        self.entries.insert(key, instance);
                    }
                } else {
                    self.entries.insert(key, instance);
                }
            }
            StackPolicy::UniqueBySource => {
                self.entries.insert(key, instance);
            }
        }
    }

    pub fn remove(&mut self, entity: EntityId, state: u32, source: EntityId) {
        self.entries.remove(&(entity, state, source.0));
    }

    pub fn get(&self, entity: EntityId, state: u32, source: EntityId) -> Option<&StateInstance> {
        self.entries.get(&(entity, state, source.0))
    }

    pub fn entity_states(&self, entity: EntityId) -> Vec<&StateInstance> {
        self.entries
            .iter()
            .filter(|((e, _, _), _)| *e == entity)
            .map(|(_, s)| s)
            .collect()
    }

    /// Remove expired states; returns the removed entities in canonical order.
    pub fn expire(&mut self, tick: Tick) -> Vec<StateInstance> {
        let expired: Vec<(EntityId, u32, u64)> = self
            .entries
            .iter()
            .filter(|(_, s)| s.is_expired(tick))
            .map(|(k, _)| *k)
            .collect();
        expired
            .into_iter()
            .map(|k| self.entries.remove(&k).expect("present"))
            .collect()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn canonical_hash_input(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        for (key, s) in &self.entries {
            buf.extend_from_slice(&key.0 .0.to_le_bytes());
            buf.extend_from_slice(&key.1.to_le_bytes());
            buf.extend_from_slice(&key.2.to_le_bytes());
            buf.extend_from_slice(&s.applied_tick.0.to_le_bytes());
            if let Some(e) = s.expires_tick {
                buf.extend_from_slice(&e.0.to_le_bytes());
            }
            buf.extend_from_slice(&s.magnitude_bp.to_le_bytes());
        }
        buf
    }
}
