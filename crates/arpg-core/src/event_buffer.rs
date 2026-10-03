use crate::events::EventOrderKey;
use crate::GameEvent;
use std::collections::BinaryHeap;

/// Deterministic event buffer: events emitted during a tick are buffered,
/// canonically ordered on drain (SPEC.md sections 12 and 162).
#[derive(Default)]
pub struct EventBuffer {
    heap: BinaryHeap<OrderedEvent>,
}

pub struct OrderedEvent {
    pub key: EventOrderKey,
    pub event: GameEvent,
}

impl PartialEq for OrderedEvent {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}
impl Eq for OrderedEvent {}
impl PartialOrd for OrderedEvent {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for OrderedEvent {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // BinaryHeap is a max-heap; order key is lexicographic, so reverse
        // to drain in ascending canonical order.
        other.key.cmp(&self.key)
    }
}

impl EventBuffer {
    pub fn new() -> EventBuffer {
        EventBuffer::default()
    }

    pub fn emit(&mut self, key: EventOrderKey, event: GameEvent) {
        self.heap.push(OrderedEvent { key, event });
    }

    pub fn drain_canonical(&mut self) -> Vec<GameEvent> {
        let mut out: Vec<OrderedEvent> = self.heap.drain().collect();
        out.sort_by_key(|e| e.key);
        out.into_iter().map(|e| e.event).collect()
    }

    pub fn len(&self) -> usize {
        self.heap.len()
    }

    pub fn is_empty(&self) -> bool {
        self.heap.is_empty()
    }
}
