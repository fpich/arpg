use crate::state::PlayerState;
use arpg_core::{PlayerId, Tick};
use std::collections::BTreeMap;

/// Replication bookkeeping (SPEC.md sections 141-143).
/// Each replicated view has a monotonically increasing revision; deltas are
/// only produced for entities whose revision moved past the client base.
#[derive(Debug, Default)]
pub struct ReplicationTracker {
    /// Latest revision per replicated entity.
    revisions: BTreeMap<PlayerId, u64>,
    /// Last revision acknowledged by each client, per entity.
    client_bases: BTreeMap<PlayerId, BTreeMap<PlayerId, u64>>,
    snapshot_counter: u64,
}

impl ReplicationTracker {
    pub fn new() -> ReplicationTracker {
        ReplicationTracker::default()
    }

    /// Record a mutation on the entity's authoritative view.
    pub fn touch(&mut self, entity: PlayerId) -> u64 {
        let rev = self.revisions.entry(entity).or_insert(0);
        *rev += 1;
        *rev
    }

    pub fn revision_of(&self, entity: PlayerId) -> u64 {
        self.revisions.get(&entity).copied().unwrap_or(0)
    }

    pub fn acknowledge(&mut self, client: PlayerId, entity: PlayerId, revision: u64) {
        self.client_bases
            .entry(client)
            .or_default()
            .insert(entity, revision);
    }

    fn client_base(&self, client: PlayerId, entity: PlayerId) -> u64 {
        self.client_bases
            .get(&client)
            .and_then(|m| m.get(&entity))
            .copied()
            .unwrap_or(0)
    }

    /// Deltas for one client relative to its acknowledged base revision.
    /// Returns (entity_id, base_revision, new_revision) for stale entities.
    pub fn deltas_for(&self, client: PlayerId) -> Vec<(PlayerId, u64, u64)> {
        let mut out = Vec::new();
        for (&entity, &rev) in &self.revisions {
            let base = self.client_base(client, entity);
            if rev > base {
                out.push((entity, base, rev));
            }
        }
        out
    }

    /// True when the client needs a full resync (base does not match server).
    pub fn needs_resync(&self, client: PlayerId, entity: PlayerId) -> bool {
        self.client_base(client, entity) != self.revision_of(entity)
    }

    pub fn next_snapshot_id(&mut self) -> u64 {
        self.snapshot_counter += 1;
        self.snapshot_counter
    }
}

/// A per-tick replication result for one client.
pub struct ClientReplication {
    pub snapshot_id: u64,
    pub tick: Tick,
    pub deltas: Vec<(PlayerId, u64, u64, PlayerState)>,
    pub resync_entities: Vec<PlayerId>,
}

pub fn build_client_replication(
    tracker: &mut ReplicationTracker,
    client: PlayerId,
    tick: Tick,
    players: &BTreeMap<PlayerId, PlayerState>,
) -> ClientReplication {
    let snapshot_id = tracker.next_snapshot_id();
    let deltas = tracker
        .deltas_for(client)
        .into_iter()
        .filter_map(|(entity, base, rev)| players.get(&entity).map(|p| (entity, base, rev, *p)))
        .collect();
    let resync_entities = tracker
        .deltas_for(client)
        .iter()
        .filter(|&(_, base, rev)| *base != 0 && rev - base > 1)
        .map(|&(entity, _, _)| entity)
        .collect();
    ClientReplication {
        snapshot_id,
        tick,
        deltas,
        resync_entities,
    }
}
