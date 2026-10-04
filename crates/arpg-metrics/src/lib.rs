//! Runtime metrics (SPEC.md section 190). Metrics are POLICY domain: they
//! never feed the state hash and never alter gameplay. No `player_id` or
//! `item_id` labels (cardinality is bounded by design): counters are
//! global, gauges carry only aggregate game scalars.

use std::collections::BTreeMap;
use std::time::Duration;

/// A named metric value at a point in time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MetricValue {
    Counter(u64),
    Gauge(f64),
    /// Sum, count, max for a duration histogram (p50/p95 computed by the
    /// observer, not stored per-sample).
    Duration {
        total_secs: f64,
        count: u64,
        max_secs: f64,
    },
}

/// Collector for the reference metrics of section 190:
/// game_tick_seconds, game_lag_ticks, game_entity_count,
/// game_monster_count, game_missile_count, commands_received,
/// commands_accepted, commands_rejected, network_bytes_sent,
/// network_bytes_received, character_save_seconds,
/// character_save_failures, item_generated, item_transaction_failures.
#[derive(Debug, Default)]
pub struct Metrics {
    counters: BTreeMap<&'static str, u64>,
    gauges: BTreeMap<&'static str, f64>,
    durations: BTreeMap<&'static str, (f64, u64, f64)>,
}

impl Metrics {
    pub fn new() -> Metrics {
        Metrics::default()
    }

    pub fn incr(&mut self, name: &'static str) {
        *self.counters.entry(name).or_insert(0) += 1;
    }

    pub fn add(&mut self, name: &'static str, delta: u64) {
        *self.counters.entry(name).or_insert(0) += delta;
    }

    pub fn set_gauge(&mut self, name: &'static str, value: f64) {
        self.gauges.insert(name, value);
    }

    pub fn observe_duration(&mut self, name: &'static str, d: Duration) {
        let secs = d.as_secs_f64();
        let e = self.durations.entry(name).or_insert((0.0, 0, 0.0));
        e.0 += secs;
        e.1 += 1;
        if secs > e.2 {
            e.2 = secs;
        }
    }

    pub fn counter(&self, name: &str) -> u64 {
        self.counters.get(name).copied().unwrap_or(0)
    }

    pub fn gauge(&self, name: &str) -> f64 {
        self.gauges.get(name).copied().unwrap_or(0.0)
    }

    pub fn duration_mean_secs(&self, name: &str) -> f64 {
        match self.durations.get(name) {
            Some((total, count, _)) if *count > 0 => total / *count as f64,
            _ => 0.0,
        }
    }

    pub fn duration_max_secs(&self, name: &str) -> f64 {
        self.durations.get(name).map(|(_, _, m)| *m).unwrap_or(0.0)
    }

    /// Snapshot in canonical (sorted) order for exporters. Metrics never
    /// carry player/item labels (section 190).
    pub fn snapshot(&self) -> Vec<(&'static str, MetricValue)> {
        let mut out = Vec::new();
        for (name, v) in &self.counters {
            out.push((*name, MetricValue::Counter(*v)));
        }
        for (name, v) in &self.gauges {
            out.push((*name, MetricValue::Gauge(*v)));
        }
        for (name, (total, count, max)) in &self.durations {
            out.push((
                *name,
                MetricValue::Duration {
                    total_secs: *total,
                    count: *count,
                    max_secs: *max,
                },
            ));
        }
        out.sort_by_key(|(name, _)| *name);
        out
    }

    /// Render a human-readable report (§167-style trace formatting).
    pub fn render(&self) -> String {
        let mut out = String::new();
        for (name, value) in self.snapshot() {
            match value {
                MetricValue::Counter(v) => out.push_str(&format!("{name} = {v}\n")),
                MetricValue::Gauge(v) => out.push_str(&format!("{name} = {v:.3}\n")),
                MetricValue::Duration {
                    total_secs,
                    count,
                    max_secs,
                } => {
                    let mean = if count > 0 {
                        total_secs / count as f64
                    } else {
                        0.0
                    };
                    out.push_str(&format!(
                        "{name} = count {count} mean {mean:.6}s max {max_secs:.6}s\n"
                    ));
                }
            }
        }
        out
    }
}

/// Names of the reference metrics (section 190).
pub mod names {
    pub const GAME_TICK_SECONDS: &str = "game_tick_seconds";
    pub const GAME_LAG_TICKS: &str = "game_lag_ticks";
    pub const GAME_ENTITY_COUNT: &str = "game_entity_count";
    pub const GAME_MONSTER_COUNT: &str = "game_monster_count";
    pub const GAME_MISSILE_COUNT: &str = "game_missile_count";
    pub const COMMANDS_RECEIVED: &str = "commands_received";
    pub const COMMANDS_ACCEPTED: &str = "commands_accepted";
    pub const COMMANDS_REJECTED: &str = "commands_rejected";
    pub const NETWORK_BYTES_SENT: &str = "network_bytes_sent";
    pub const NETWORK_BYTES_RECEIVED: &str = "network_bytes_received";
    pub const CHARACTER_SAVE_SECONDS: &str = "character_save_seconds";
    pub const CHARACTER_SAVE_FAILURES: &str = "character_save_failures";
    pub const ITEM_GENERATED: &str = "item_generated";
    pub const ITEM_TRANSACTION_FAILURES: &str = "item_transaction_failures";
    pub const INVALID_ITEM_LOCATION: &str = "invalid_item_location";
    pub const STATE_HASH_MISMATCH: &str = "state_hash_mismatch";
    pub const REVISION_CONFLICT: &str = "revision_conflict";
    pub const SCHEDULER_OVERFLOW: &str = "scheduler_overflow";
    pub const GENERATION_RETRY: &str = "generation_retry";
    pub const RESYNC_REQUESTED: &str = "resync_requested";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counters_accumulate() {
        let mut m = Metrics::new();
        m.incr(names::COMMANDS_RECEIVED);
        m.add(names::COMMANDS_RECEIVED, 4);
        m.incr(names::COMMANDS_ACCEPTED);
        assert_eq!(m.counter(names::COMMANDS_RECEIVED), 5);
        assert_eq!(m.counter(names::COMMANDS_ACCEPTED), 1);
        assert_eq!(m.counter(names::COMMANDS_REJECTED), 0);
    }

    #[test]
    fn gauges_track_latest_value() {
        let mut m = Metrics::new();
        m.set_gauge(names::GAME_MONSTER_COUNT, 12.0);
        m.set_gauge(names::GAME_MONSTER_COUNT, 15.0);
        assert_eq!(m.gauge(names::GAME_MONSTER_COUNT), 15.0);
    }

    #[test]
    fn durations_compute_mean_and_max() {
        let mut m = Metrics::new();
        m.observe_duration(names::GAME_TICK_SECONDS, Duration::from_millis(2));
        m.observe_duration(names::GAME_TICK_SECONDS, Duration::from_millis(6));
        assert!((m.duration_mean_secs(names::GAME_TICK_SECONDS) - 0.004).abs() < 1e-9);
        assert!((m.duration_max_secs(names::GAME_TICK_SECONDS) - 0.006).abs() < 1e-9);
    }

    #[test]
    fn snapshot_is_canonical_and_label_free() {
        let mut m = Metrics::new();
        m.incr(names::ITEM_GENERATED);
        m.set_gauge(names::GAME_ENTITY_COUNT, 42.0);
        let snap = m.snapshot();
        assert_eq!(snap.len(), 2);
        let names: Vec<&str> = snap.iter().map(|(n, _)| *n).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        assert_eq!(names, sorted, "snapshot is in canonical order");
        for (name, _) in &snap {
            assert!(!name.contains("player"), "no player labels (section 190)");
            assert!(!name.contains("item_id"), "no item labels (section 190)");
        }
    }
}
