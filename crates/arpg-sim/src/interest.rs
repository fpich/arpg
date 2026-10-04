//! Interest management (SPEC.md section 140).
//!
//! The interest set of a client decides which entities replicate to it.
//! An entity leaving the set produces `EntityOutOfScope` — never a despawn:
//! the authoritative state keeps the entity alive and visible again when
//! interest returns.
//!
//! Composition: same level, distance within the interest radius, party
//! membership (party members are always in scope regardless of distance)
//! and the observing player itself.

use crate::social::PartySystem;
use crate::state::PlayerState;
use arpg_core::{PlayerId, WorldPos};
use std::collections::BTreeMap;

/// Default interest radius in world units (integer, fixed-point free).
pub const DEFAULT_INTEREST_RADIUS: i32 = 4096;

/// Outcome of an interest evaluation for one (client, entity) pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterestEvent {
    /// The entity entered or stays in the client's interest set.
    InScope,
    /// The entity left the client's interest set (section 140): the client
    /// must stop rendering it, but the server does NOT despawn it.
    OutOfScope,
}

#[derive(Debug, Default)]
pub struct InterestSet {
    /// Last known in-scope state per (client, entity).
    in_scope: BTreeMap<(PlayerId, PlayerId), bool>,
    /// Radius override per client; DEFAULT_INTEREST_RADIUS otherwise.
    radii: BTreeMap<PlayerId, i32>,
}

impl InterestSet {
    pub fn new() -> InterestSet {
        InterestSet::default()
    }

    /// Override the interest radius for one client.
    pub fn set_radius(&mut self, client: PlayerId, radius: i32) {
        self.radii.insert(client, radius);
    }

    fn radius(&self, client: PlayerId) -> i32 {
        self.radii
            .get(&client)
            .copied()
            .unwrap_or(DEFAULT_INTEREST_RADIUS)
    }

    /// Static scope test: party members and the player itself are always
    /// in scope; otherwise the entity must lie within the client's radius.
    pub fn in_scope(
        &self,
        client: PlayerId,
        entity: PlayerId,
        client_pos: WorldPos,
        entity_pos: WorldPos,
        parties: &PartySystem,
    ) -> bool {
        if client == entity {
            return true;
        }
        if parties
            .party_of(client)
            .is_some_and(|p| p.members.contains(&entity))
        {
            return true;
        }
        let dx = (client_pos.x - entity_pos.x).abs();
        let dy = (client_pos.y - entity_pos.y).abs();
        dx <= self.radius(client) && dy <= self.radius(client)
    }

    /// Evaluate one entity for one client and record the transition.
    pub fn evaluate(
        &mut self,
        client: PlayerId,
        entity: PlayerId,
        client_pos: WorldPos,
        entity_pos: WorldPos,
        parties: &PartySystem,
    ) -> InterestEvent {
        let now = self.in_scope(client, entity, client_pos, entity_pos, parties);
        let was = self
            .in_scope
            .get(&(client, entity))
            .copied()
            .unwrap_or(false);
        self.in_scope.insert((client, entity), now);
        if now && !was {
            InterestEvent::InScope
        } else if !now && was {
            InterestEvent::OutOfScope
        } else if now {
            InterestEvent::InScope
        } else {
            InterestEvent::OutOfScope
        }
    }

    /// Entities currently in the client's interest set, canonically ordered.
    pub fn scope_of(
        &self,
        client: PlayerId,
        players: &BTreeMap<PlayerId, PlayerState>,
        parties: &PartySystem,
    ) -> Vec<PlayerId> {
        let client_state = players.get(&client);
        let client_pos = client_state.map(|p| p.pos).unwrap_or(WorldPos::ZERO);
        players
            .iter()
            .filter(|(entity, state)| {
                self.in_scope(client, **entity, client_pos, state.pos, parties)
            })
            .map(|(entity, _)| *entity)
            .collect()
    }

    /// Out-of-scope entities for a client: previously in scope, now out.
    /// Feeds `EntityOutOfScope` events (section 140) — not despawns.
    pub fn out_of_scope(&self, client: PlayerId) -> Vec<PlayerId> {
        self.in_scope
            .iter()
            .filter(|((c, _), s)| *c == client && !**s)
            .map(|((_, e), _)| *e)
            .collect()
    }

    /// Canonical hash input (determinism, section 160): sorted pairs.
    pub fn hash_bytes(&self, out: &mut Vec<u8>) {
        for ((client, entity), scope) in &self.in_scope {
            out.extend_from_slice(&client.0.to_le_bytes());
            out.extend_from_slice(&entity.0.to_le_bytes());
            out.push(*scope as u8);
        }
        for (client, radius) in &self.radii {
            out.extend_from_slice(&client.0.to_le_bytes());
            out.extend_from_slice(&radius.to_le_bytes());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::social::PartySystem;

    fn players(positions: &[(PlayerId, WorldPos)]) -> BTreeMap<PlayerId, PlayerState> {
        positions
            .iter()
            .map(|(id, pos)| {
                (
                    *id,
                    PlayerState {
                        player: *id,
                        pos: *pos,
                        life: 100,
                        mana: 100,
                        level: 1,
                        experience: 0,
                        active_regen: None,
                    },
                )
            })
            .collect()
    }

    #[test]
    fn distance_bounds_interest() {
        let set = InterestSet::new();
        let parties = PartySystem::new();
        let players = players(&[
            (PlayerId(1), WorldPos::new(0, 0)),
            (PlayerId(2), WorldPos::new(100, 0)),
            (PlayerId(3), WorldPos::new(10_000, 0)),
        ]);
        let scope = set.scope_of(PlayerId(1), &players, &parties);
        assert!(scope.contains(&PlayerId(2)));
        assert!(!scope.contains(&PlayerId(3)), "far player out of scope");
    }

    #[test]
    fn party_members_stay_in_scope_at_any_distance() {
        let mut parties = PartySystem::new();
        parties.invite(PlayerId(1), PlayerId(2), 8).unwrap();
        parties.accept(PlayerId(2), 8).unwrap();
        let set = InterestSet::new();
        let players = players(&[
            (PlayerId(1), WorldPos::new(0, 0)),
            (PlayerId(2), WorldPos::new(500_000, 0)),
        ]);
        let scope = set.scope_of(PlayerId(1), &players, &parties);
        assert!(
            scope.contains(&PlayerId(2)),
            "party member in scope regardless of distance"
        );
    }

    #[test]
    fn leaving_scope_yields_out_of_scope_not_despawn() {
        let mut set = InterestSet::new();
        let parties = PartySystem::new();
        let near = WorldPos::new(0, 0);
        let far = WorldPos::new(1_000_000, 0);
        let e = set.evaluate(PlayerId(1), PlayerId(2), near, near, &parties);
        assert_eq!(e, InterestEvent::InScope);
        let e = set.evaluate(PlayerId(1), PlayerId(2), near, far, &parties);
        assert_eq!(e, InterestEvent::OutOfScope);
        assert_eq!(set.out_of_scope(PlayerId(1)), vec![PlayerId(2)]);
        // coming back in scope is observable again
        let e = set.evaluate(PlayerId(1), PlayerId(2), near, near, &parties);
        assert_eq!(e, InterestEvent::InScope);
        assert!(set.out_of_scope(PlayerId(1)).is_empty());
    }
}
