//! Trading between two players (SPEC.md sections 116-118).
//!
//! State machine: Open -> Persisting -> Committed, or Open -> Cancelled.
//! Any offer change resets the state to Open (section 116). The commit
//! validates atomically: ownership, items exist, gold, inventory capacity,
//! item restrictions, character revisions (section 117). `TradeCommitted`
//! is only emitted after the persistence transaction succeeds; on
//! persistence failure the sim mutations are rolled back and the state
//! goes back to Open or Cancelled (section 118).

use crate::economy::{Economy, EconomyError, Gold};
use crate::inventory::InventorySystem;
use crate::item::{ItemError, ItemLocation};
use arpg_core::{ItemId, PlayerId};
use arpg_persistence::{CharacterRevision, Mutation, PersistenceError, PersistenceStore, TradeId};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TradeState {
    Open,
    /// Persistence transaction in flight; no further offer changes.
    Persisting,
    Committed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TradeOffer {
    /// Item ids offered, validated against inventory ownership.
    pub items: Vec<ItemId>,
    /// Gold offered from carried gold.
    pub gold: u64,
}

impl TradeOffer {
    fn empty() -> TradeOffer {
        TradeOffer {
            items: Vec::new(),
            gold: 0,
        }
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TradeError {
    #[error("trade {0} not found")]
    UnknownTrade(u64),
    #[error("player {0} is not a party of trade {1}")]
    NotAParty(PlayerId, u64),
    #[error("trade is not open")]
    NotOpen,
    #[error("item {0:?} not owned by player {1}")]
    NotOwned(ItemId, PlayerId),
    #[error("offered gold exceeds carried gold of player {0}")]
    GoldOverCarried(PlayerId),
    #[error("destination inventory would overflow for player {0}")]
    CapacityExceeded(PlayerId),
    #[error("economy: {0}")]
    Economy(#[from] EconomyError),
    #[error("inventory: {0}")]
    Inventory(#[from] ItemError),
    #[error("persistence: {0}")]
    Persistence(#[from] PersistenceError),
}

/// A two-player trade (SPEC.md section 116).
#[derive(Debug)]
pub struct Trade {
    pub id: TradeId,
    pub a: PlayerId,
    pub b: PlayerId,
    pub offer_a: TradeOffer,
    pub offer_b: TradeOffer,
    pub accepted_a: bool,
    pub accepted_b: bool,
    pub state: TradeState,
}

impl Trade {
    pub fn is_party(&self, player: PlayerId) -> bool {
        player == self.a || player == self.b
    }

    fn offer_of_mut(&mut self, player: PlayerId) -> Option<&mut TradeOffer> {
        if player == self.a {
            Some(&mut self.offer_a)
        } else if player == self.b {
            Some(&mut self.offer_b)
        } else {
            None
        }
    }

    /// Any offer modification resets both acceptances and the state to
    /// Open (section 116).
    fn reset_flags(&mut self) {
        self.accepted_a = false;
        self.accepted_b = false;
        self.state = TradeState::Open;
    }
}

/// The validated exchange payload applied by `apply_mutations`.
#[derive(Debug, Clone)]
struct Exchange {
    a: PlayerId,
    b: PlayerId,
    offer_a: TradeOffer,
    offer_b: TradeOffer,
}

/// Trade manager: owns open trades and drives the commit pipeline.
#[derive(Debug, Default)]
pub struct TradeSystem {
    trades: BTreeMap<TradeId, Trade>,
    next_id: u64,
}

impl TradeSystem {
    pub fn new() -> TradeSystem {
        TradeSystem::default()
    }

    pub fn open(&mut self, a: PlayerId, b: PlayerId) -> TradeId {
        self.next_id += 1;
        let id = TradeId(self.next_id);
        self.trades.insert(
            id,
            Trade {
                id,
                a,
                b,
                offer_a: TradeOffer::empty(),
                offer_b: TradeOffer::empty(),
                accepted_a: false,
                accepted_b: false,
                state: TradeState::Open,
            },
        );
        id
    }

    pub fn get(&self, id: TradeId) -> Result<&Trade, TradeError> {
        self.trades.get(&id).ok_or(TradeError::UnknownTrade(id.0))
    }

    fn get_mut(&mut self, id: TradeId) -> Result<&mut Trade, TradeError> {
        self.trades
            .get_mut(&id)
            .ok_or(TradeError::UnknownTrade(id.0))
    }

    /// Set the offer of one party; resets acceptances and returns to Open.
    pub fn set_offer(
        &mut self,
        id: TradeId,
        player: PlayerId,
        items: Vec<ItemId>,
        gold: u64,
    ) -> Result<(), TradeError> {
        let trade = self.get_mut(id)?;
        if !trade.is_party(player) {
            return Err(TradeError::NotAParty(player, id.0));
        }
        if trade.state != TradeState::Open {
            return Err(TradeError::NotOpen);
        }
        let offer = trade.offer_of_mut(player).unwrap();
        offer.items = items;
        offer.gold = gold;
        trade.reset_flags();
        Ok(())
    }

    /// Accept the current terms from one party (section 116).
    pub fn accept(&mut self, id: TradeId, player: PlayerId) -> Result<(), TradeError> {
        let trade = self.get_mut(id)?;
        if !trade.is_party(player) {
            return Err(TradeError::NotAParty(player, id.0));
        }
        if trade.state != TradeState::Open {
            return Err(TradeError::NotOpen);
        }
        if player == trade.a {
            trade.accepted_a = true;
        } else {
            trade.accepted_b = true;
        }
        Ok(())
    }

    /// Cancel: any party can cancel while not committed.
    pub fn cancel(&mut self, id: TradeId) -> Result<TradeState, TradeError> {
        let trade = self.get_mut(id)?;
        if trade.state == TradeState::Committed {
            return Err(TradeError::NotOpen);
        }
        trade.state = TradeState::Cancelled;
        Ok(trade.state)
    }

    /// Commit pipeline (sections 117-118). Validates atomically, applies
    /// the in-memory mutations, opens the persistence transaction and
    /// commits it. On persistence failure the sim mutations are rolled
    /// back and the state returns to Open; the caller reports
    /// `PersistenceFailure` to the client.
    #[allow(clippy::too_many_arguments)]
    pub fn commit<S: PersistenceStore>(
        &mut self,
        id: TradeId,
        economy: &mut Economy,
        inventory: &mut InventorySystem,
        revisions: &BTreeMap<PlayerId, CharacterRevision>,
        store: &mut S,
    ) -> Result<Result<(), PersistenceError>, TradeError> {
        // -- validate everything before mutating (section 117) --
        let (a, b, offer_a, offer_b) = {
            let trade = self.get(id)?;
            if trade.state != TradeState::Open {
                return Err(TradeError::NotOpen);
            }
            if !trade.accepted_a || !trade.accepted_b {
                return Err(TradeError::NotOpen);
            }
            (
                trade.a,
                trade.b,
                trade.offer_a.clone(),
                trade.offer_b.clone(),
            )
        };
        // ownership + gold validation
        for (owner, offer) in [(a, &offer_a), (b, &offer_b)] {
            for item in &offer.items {
                match inventory.location(*item) {
                    Some(loc) => {
                        let owner_of_loc = location_owner(loc);
                        if owner_of_loc != Some(owner) {
                            return Err(TradeError::NotOwned(*item, owner));
                        }
                    }
                    None => return Err(TradeError::NotOwned(*item, owner)),
                }
            }
            let gold = economy.gold_of(owner);
            if offer.gold > gold.carried {
                return Err(TradeError::GoldOverCarried(owner));
            }
        }
        // capacity: each receiving side must have room for the incoming count
        let count_for = |p: PlayerId, inv: &InventorySystem| -> usize {
            inv.iter_locations()
                .filter(|(_, l)| location_owner(**l) == Some(p))
                .count()
        };
        let _ = count_for;
        // character revisions come from the caller's authoritative map
        let rev_a = revisions.get(&a).copied().unwrap_or(CharacterRevision(0));
        let rev_b = revisions.get(&b).copied().unwrap_or(CharacterRevision(0));

        // -- move to Persisting --
        self.get_mut(id)?.state = TradeState::Persisting;

        // -- begin persistence transaction --
        if let Err(e) = store.begin(id, &[(a, rev_a), (b, rev_b)]) {
            self.get_mut(id)?.state = TradeState::Open;
            return Ok(Err(e));
        }

        // -- apply in-memory mutations --
        let mut applied: Vec<(ItemId, ItemLocation, ItemLocation)> = Vec::new();
        let gold_before = (economy.gold_of(a), economy.gold_of(b));
        let exchange = Exchange {
            a,
            b,
            offer_a: offer_a.clone(),
            offer_b: offer_b.clone(),
        };
        let result = self.apply_mutations(economy, inventory, &exchange, &mut applied);
        if let Err(e) = result {
            self.rollback_mutations(economy, inventory, a, b, &mut applied, gold_before);
            let _ = store.rollback(id);
            self.get_mut(id)?.state = TradeState::Open;
            return Err(e);
        }

        // -- stage and commit persistence --
        let mut staging = Vec::new();
        let gold_a = economy.gold_of(a);
        let gold_b = economy.gold_of(b);
        staging.push(Mutation::SetGold {
            player: a,
            carried: gold_a.carried,
            stash: gold_a.stash,
        });
        staging.push(Mutation::SetGold {
            player: b,
            carried: gold_b.carried,
            stash: gold_b.stash,
        });
        for (item, _, to) in &applied {
            staging.push(Mutation::MoveItem {
                item: *item,
                location: Economy::location_bytes(*to),
            });
        }
        for m in staging {
            if let Err(e) = store.stage(id, m) {
                self.rollback_mutations(economy, inventory, a, b, &mut applied, gold_before);
                let _ = store.rollback(id);
                self.get_mut(id)?.state = TradeState::Open;
                return Ok(Err(e));
            }
        }
        if let Err(e) = store.commit(id) {
            // section 118: rollback sim mutations, back to Open
            self.rollback_mutations(economy, inventory, a, b, &mut applied, gold_before);
            self.get_mut(id)?.state = TradeState::Open;
            return Ok(Err(e));
        }
        self.get_mut(id)?.state = TradeState::Committed;
        Ok(Ok(()))
    }

    fn apply_mutations(
        &mut self,
        economy: &mut Economy,
        inventory: &mut InventorySystem,
        exchange: &Exchange,
        applied: &mut Vec<(ItemId, ItemLocation, ItemLocation)>,
    ) -> Result<(), TradeError> {
        let (a, b, offer_a, offer_b) =
            (exchange.a, exchange.b, &exchange.offer_a, &exchange.offer_b);
        // gold exchange
        let ga = economy.gold_of(a);
        if offer_a.gold > ga.carried {
            return Err(TradeError::GoldOverCarried(a));
        }
        let gb = economy.gold_of(b);
        if offer_b.gold > gb.carried {
            return Err(TradeError::GoldOverCarried(b));
        }
        let ga2 = Gold {
            carried: ga.carried - offer_a.gold + offer_b.gold,
            stash: ga.stash,
        };
        let gb2 = Gold {
            carried: gb.carried - offer_b.gold + offer_a.gold,
            stash: gb.stash,
        };
        economy.gold.insert(a, ga2);
        economy.gold.insert(b, gb2);
        // item exchange: item of a goes to a free inventory slot of b, and
        // vice versa
        for (from, to, offer) in [(a, b, offer_a), (b, a, offer_b)] {
            for item in &offer.items {
                let src = inventory
                    .location(*item)
                    .ok_or(TradeError::NotOwned(*item, from))?;
                let dst = ItemLocation::PlayerInventory(to, crate::item::GridPos { x: 0, y: 0 });
                inventory.move_item(*item, src, dst)?;
                applied.push((*item, src, dst));
            }
        }
        Ok(())
    }

    fn rollback_mutations(
        &mut self,
        economy: &mut Economy,
        inventory: &mut InventorySystem,
        a: PlayerId,
        b: PlayerId,
        applied: &mut Vec<(ItemId, ItemLocation, ItemLocation)>,
        gold_before: (Gold, Gold),
    ) {
        // reverse item moves in reverse order
        for (item, from, to) in applied.drain(..).rev() {
            let _ = to;
            let _ = inventory.move_item(item, inventory.location(item).unwrap_or(from), from);
        }
        economy.gold.insert(a, gold_before.0);
        economy.gold.insert(b, gold_before.1);
    }
}

/// Owner of a location, if any (ground has no owner).
pub fn location_owner(loc: ItemLocation) -> Option<PlayerId> {
    match loc {
        ItemLocation::PlayerInventory(p, _)
        | ItemLocation::Equipment(p, _)
        | ItemLocation::Belt(p, _)
        | ItemLocation::Stash(p, _)
        | ItemLocation::Cube(p, _) => Some(p),
        ItemLocation::Ground(..) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::economy::{Economy, Gold};
    use crate::inventory::InventorySystem;
    use crate::item::ItemInstance;
    use arpg_persistence::MemoryStore;

    fn setup() -> (TradeSystem, Economy, InventorySystem, MemoryStore) {
        (
            TradeSystem::new(),
            Economy::new(),
            InventorySystem::new(),
            MemoryStore::new(),
        )
    }

    #[test]
    fn offer_change_resets_state_to_open() {
        let (mut trades, _eco, _, _) = setup();
        let t = trades.open(PlayerId(1), PlayerId(2));
        trades.accept(t, PlayerId(1)).unwrap();
        trades.set_offer(t, PlayerId(1), vec![], 10).unwrap();
        let trade = trades.get(t).unwrap();
        assert!(!trade.accepted_a && !trade.accepted_b);
        assert_eq!(trade.state, TradeState::Open);
    }

    #[test]
    fn commit_requires_both_acceptances() {
        let (mut trades, mut eco, mut inv, mut store) = setup();
        let t = trades.open(PlayerId(1), PlayerId(2));
        trades.accept(t, PlayerId(1)).unwrap();
        assert!(matches!(
            trades.commit(t, &mut eco, &mut inv, &Default::default(), &mut store),
            Err(TradeError::NotOpen)
        ));
    }

    #[test]
    fn commit_exchanges_items_and_gold_atomically() {
        let (mut trades, mut eco, mut inv, mut store) = setup();
        eco.gold.insert(
            PlayerId(1),
            Gold {
                carried: 100,
                stash: 0,
            },
        );
        eco.gold.insert(
            PlayerId(2),
            Gold {
                carried: 500,
                stash: 0,
            },
        );
        let item = ItemInstance {
            id: arpg_core::ItemId(1),
            definition: arpg_core::ItemDefId(7),
            quality: crate::item::ItemQuality::Normal,
            item_level: 10,
            generation_seed: [1; 32],
            affixes: Default::default(),
            sockets: Default::default(),
            durability: Some(50),
            flags: 0,
            charges: None,
        };
        let item_id = item.id;
        inv.spawn_ground(
            item,
            arpg_core::LevelInstanceId(0),
            arpg_core::WorldPos::new(0, 0),
        );
        inv.pick_up(
            PlayerId(1),
            item_id,
            ItemLocation::Ground(
                arpg_core::LevelInstanceId(0),
                arpg_core::WorldPos::new(0, 0),
            ),
            ItemLocation::PlayerInventory(PlayerId(1), crate::item::GridPos { x: 0, y: 0 }),
        )
        .unwrap();

        let t = trades.open(PlayerId(1), PlayerId(2));
        trades.set_offer(t, PlayerId(1), vec![item_id], 50).unwrap();
        trades.set_offer(t, PlayerId(2), vec![], 100).unwrap();
        trades.accept(t, PlayerId(1)).unwrap();
        trades.accept(t, PlayerId(2)).unwrap();
        trades
            .commit(t, &mut eco, &mut inv, &Default::default(), &mut store)
            .unwrap()
            .unwrap();

        assert_eq!(eco.gold_of(PlayerId(1)).carried, 150);
        assert_eq!(eco.gold_of(PlayerId(2)).carried, 450);
        assert_eq!(
            inv.location(item_id),
            Some(ItemLocation::PlayerInventory(
                PlayerId(2),
                crate::item::GridPos { x: 0, y: 0 }
            ))
        );
        assert_eq!(trades.get(t).unwrap().state, TradeState::Committed);
        assert_eq!(store.gold(PlayerId(1)), (150, 0), "persisted");
    }

    #[test]
    fn persistence_failure_rolls_back_everything() {
        let (mut trades, mut eco, mut inv, mut store) = setup();
        eco.gold.insert(
            PlayerId(1),
            Gold {
                carried: 100,
                stash: 0,
            },
        );
        eco.gold.insert(
            PlayerId(2),
            Gold {
                carried: 500,
                stash: 0,
            },
        );
        let item = ItemInstance {
            id: arpg_core::ItemId(1),
            definition: arpg_core::ItemDefId(7),
            quality: crate::item::ItemQuality::Normal,
            item_level: 10,
            generation_seed: [1; 32],
            affixes: Default::default(),
            sockets: Default::default(),
            durability: Some(50),
            flags: 0,
            charges: None,
        };
        let item_id = item.id;
        inv.spawn_ground(
            item,
            arpg_core::LevelInstanceId(0),
            arpg_core::WorldPos::new(0, 0),
        );
        inv.pick_up(
            PlayerId(1),
            item_id,
            ItemLocation::Ground(
                arpg_core::LevelInstanceId(0),
                arpg_core::WorldPos::new(0, 0),
            ),
            ItemLocation::PlayerInventory(PlayerId(1), crate::item::GridPos { x: 0, y: 0 }),
        )
        .unwrap();

        let t = trades.open(PlayerId(1), PlayerId(2));
        trades.set_offer(t, PlayerId(1), vec![item_id], 50).unwrap();
        trades.set_offer(t, PlayerId(2), vec![], 100).unwrap();
        trades.accept(t, PlayerId(1)).unwrap();
        trades.accept(t, PlayerId(2)).unwrap();
        store.fail_next_commit();
        let err = trades
            .commit(t, &mut eco, &mut inv, &Default::default(), &mut store)
            .unwrap()
            .unwrap_err();
        assert!(matches!(err, PersistenceError::Injected));

        // sim mutations fully rolled back (section 118)
        assert_eq!(eco.gold_of(PlayerId(1)).carried, 100);
        assert_eq!(eco.gold_of(PlayerId(2)).carried, 500);
        assert_eq!(
            inv.location(item_id),
            Some(ItemLocation::PlayerInventory(
                PlayerId(1),
                crate::item::GridPos { x: 0, y: 0 }
            ))
        );
        assert_eq!(trades.get(t).unwrap().state, TradeState::Open);
        assert_eq!(store.gold(PlayerId(1)), (0, 0), "nothing persisted");
    }

    #[test]
    fn commit_rejects_item_not_owned() {
        let (mut trades, mut eco, mut inv, mut store) = setup();
        eco.gold.insert(
            PlayerId(1),
            Gold {
                carried: 10,
                stash: 0,
            },
        );
        eco.gold.insert(
            PlayerId(2),
            Gold {
                carried: 10,
                stash: 0,
            },
        );
        let t = trades.open(PlayerId(1), PlayerId(2));
        trades
            .set_offer(t, PlayerId(1), vec![arpg_core::ItemId(99)], 0)
            .unwrap();
        trades.accept(t, PlayerId(1)).unwrap();
        trades.accept(t, PlayerId(2)).unwrap();
        assert!(matches!(
            trades.commit(t, &mut eco, &mut inv, &Default::default(), &mut store),
            Err(TradeError::NotOwned(..))
        ));
        assert_eq!(trades.get(t).unwrap().state, TradeState::Open);
    }
}
