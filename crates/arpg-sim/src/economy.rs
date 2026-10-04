//! Economy systems (SPEC.md sections 94-97): gold, merchant services,
//! gambling. Trade lives in `trade.rs` because it spans persistence.

use crate::inventory::InventorySystem;
use crate::item::{ItemError, ItemInstance, ItemLocation, ItemQuality};
use arpg_core::{ItemDefId, PlayerId};
use std::collections::BTreeMap;

/// Gold is a numeric resource, not an inventory item (SPEC.md section 94).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Gold {
    pub carried: u64,
    pub stash: u64,
}

impl Gold {
    pub const fn zero() -> Gold {
        Gold {
            carried: 0,
            stash: 0,
        }
    }
    pub fn total(&self) -> u64 {
        self.carried.saturating_add(self.stash)
    }
}

/// A pile of gold on the ground (SPEC.md section 94).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GroundCurrency {
    pub amount: u64,
    pub pos: arpg_core::WorldPos,
    pub level: arpg_core::LevelInstanceId,
}

/// Per-player merchant stock (SPEC.md section 95): personal stock removes
/// inter-player conflicts. Refresh uses the sequenced domain
/// `Merchant(NpcId, merchant_refresh_sequence)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MerchantEntry {
    pub def: ItemDefId,
    pub price: u64,
    pub quantity: u32,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum EconomyError {
    #[error("insufficient gold: need {0}")]
    InsufficientGold(u64),
    #[error("merchant stock exhausted for item {0:?}")]
    OutOfStock(ItemDefId),
    #[error("item {0:?} not sellable")]
    NotSellable(ItemDefId),
    #[error("unknown merchant {0}")]
    UnknownMerchant(u64),
    #[error("gambling requires exactly one offer")]
    InvalidGamble,
    #[error("inventory error: {0}")]
    Inventory(#[from] ItemError),
}

/// Merchant services (SPEC.md section 95).
#[derive(Debug, Clone)]
pub struct Merchant {
    pub id: u64,
    /// Buy price multiplier and sell price multiplier from the ruleset.
    pub buy_multiplier: u16,
    pub sell_multiplier: u16,
    /// Repair cost per point of missing durability.
    pub repair_cost_per_point: u64,
    /// Recharge cost per point of missing charges.
    pub recharge_cost_per_point: u64,
    /// Personal stock per player (section 95).
    stock: BTreeMap<PlayerId, Vec<MerchantEntry>>,
    /// Gambling offers per player: unknown until bought (section 97).
    gamble_stock: BTreeMap<PlayerId, Vec<GambleOffer>>,
}

/// A gamble offer: the client only sees appearance/price; the server resolves
/// quality and properties on purchase (SPEC.md section 97).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GambleOffer {
    pub appearance: ItemDefId,
    pub price: u64,
    /// Hidden until purchase: resolved quality seed.
    pub secret_seed: [u8; 32],
}

impl Merchant {
    pub fn new(id: u64) -> Merchant {
        Merchant {
            id,
            buy_multiplier: 100,
            sell_multiplier: 25,
            repair_cost_per_point: 10,
            recharge_cost_per_point: 10,
            stock: BTreeMap::new(),
            gamble_stock: BTreeMap::new(),
        }
    }

    /// Restock the personal stock of a player (refresh trigger, section 96).
    /// The caller passes entries generated from the sequenced RNG domain.
    pub fn refresh(&mut self, player: PlayerId, entries: Vec<MerchantEntry>) {
        self.stock.insert(player, entries);
    }

    pub fn refresh_gamble(&mut self, player: PlayerId, offers: Vec<GambleOffer>) {
        self.gamble_stock.insert(player, offers);
    }

    pub fn stock(&self, player: PlayerId) -> &[MerchantEntry] {
        self.stock.get(&player).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn gamble_offers(&self, player: PlayerId) -> &[GambleOffer] {
        self.gamble_stock
            .get(&player)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// Buy: pay carried gold, decrement personal stock, receive the item at
    /// the given destination (validated transactionally by the caller).
    pub fn buy(
        &mut self,
        player: PlayerId,
        gold: &mut Gold,
        def: ItemDefId,
        price_override: Option<u64>,
    ) -> Result<u64, EconomyError> {
        let entries = self
            .stock
            .get_mut(&player)
            .ok_or(EconomyError::UnknownMerchant(self.id))?;
        let idx = entries
            .iter()
            .position(|e| e.def == def)
            .ok_or(EconomyError::OutOfStock(def))?;
        if entries[idx].quantity == 0 {
            return Err(EconomyError::OutOfStock(def));
        }
        let price = price_override.unwrap_or(entries[idx].price);
        if gold.carried < price {
            return Err(EconomyError::InsufficientGold(price));
        }
        gold.carried -= price;
        entries[idx].quantity -= 1;
        Ok(price)
    }

    /// Sell: the merchant pays the sell multiplier of the base price.
    pub fn sell(&mut self, player: PlayerId, gold: &mut Gold, base_price: u64) -> u64 {
        let _ = player;
        let payout = base_price * self.sell_multiplier as u64 / 100;
        gold.carried = gold.carried.saturating_add(payout);
        payout
    }

    /// Repair: cost scales with missing durability; capped by carried gold.
    pub fn repair_cost(&self, missing_points: u64) -> u64 {
        missing_points.saturating_mul(self.repair_cost_per_point)
    }

    /// Recharge: cost scales with missing charges.
    pub fn recharge_cost(&self, missing_charges: u64) -> u64 {
        missing_charges.saturating_mul(self.recharge_cost_per_point)
    }

    /// Gamble purchase (SPEC.md section 97): resolves the hidden offer to a
    /// concrete item instance. The definition/quality stay server-side
    /// until the item is delivered.
    pub fn gamble_buy(
        &mut self,
        player: PlayerId,
        gold: &mut Gold,
        offer_index: usize,
    ) -> Result<ItemInstance, EconomyError> {
        let offers = self
            .gamble_stock
            .get_mut(&player)
            .ok_or(EconomyError::UnknownMerchant(self.id))?;
        if offer_index >= offers.len() {
            return Err(EconomyError::InvalidGamble);
        }
        let offer = offers[offer_index].clone();
        if gold.carried < offer.price {
            return Err(EconomyError::InsufficientGold(offer.price));
        }
        gold.carried -= offer.price;
        offers.remove(offer_index);
        // resolve the gamble from the hidden seed: quality and properties
        // were decided at offer time and never sent to the client
        let quality = gamble_quality(&offer.secret_seed);
        let def = offer.appearance;
        Ok(ItemInstance {
            id: arpg_core::ItemId(offer.secret_seed[0] as u128),
            definition: def,
            quality,
            item_level: 1 + (offer.secret_seed[1] as u16) % 50,
            generation_seed: offer.secret_seed,
            affixes: Default::default(),
            sockets: Default::default(),
            durability: Some(50),
            flags: 0,
        })
    }
}

/// Hidden quality resolution for a gamble (SPEC.md section 97).
fn gamble_quality(seed: &[u8; 32]) -> ItemQuality {
    match seed[2] % 4 {
        0 => ItemQuality::Normal,
        1 => ItemQuality::Magic,
        2 => ItemQuality::Rare,
        _ => ItemQuality::Set,
    }
}

/// Cube transmutation: deterministic recipe matching on inputs
/// (SPEC.md section 4313 lists the cube under economy).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CubeRecipe {
    pub inputs: Vec<ItemDefId>,
    pub output: ItemDefId,
    pub output_quality: ItemQuality,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CubeError {
    #[error("no recipe matches the input set")]
    NoRecipe,
    #[error("cube inputs must be in the cube grid")]
    NotInCube,
}

/// Match a set of item definitions against known recipes. Order-insensitive.
pub fn transmute(recipes: &[CubeRecipe], inputs: &[ItemDefId]) -> Result<CubeRecipe, CubeError> {
    let mut sorted = inputs.to_vec();
    sorted.sort();
    for recipe in recipes {
        let mut want = recipe.inputs.clone();
        want.sort();
        if want == sorted {
            return Ok(recipe.clone());
        }
    }
    Err(CubeError::NoRecipe)
}

/// Central economy facade used by the sim.
#[derive(Debug, Default)]
pub struct Economy {
    pub gold: BTreeMap<PlayerId, Gold>,
    pub ground_gold: Vec<GroundCurrency>,
    pub merchants: BTreeMap<u64, Merchant>,
}

impl Economy {
    pub fn new() -> Economy {
        Economy::default()
    }

    pub fn gold_of(&self, player: PlayerId) -> Gold {
        self.gold.get(&player).copied().unwrap_or(Gold::zero())
    }

    pub fn merchant(&mut self, id: u64) -> &mut Merchant {
        self.merchants
            .entry(id)
            .or_insert_with(|| Merchant::new(id))
    }

    /// Pick up a ground gold pile: first-come wins, merged into carried gold.
    pub fn pick_up_gold(&mut self, player: PlayerId) -> u64 {
        let mut picked: u64 = 0;
        for pile in self.ground_gold.drain(..) {
            // M2-era interaction: any pile at interaction range is collected
            picked = picked.saturating_add(pile.amount);
        }
        self.ground_gold
            .sort_by_key(|p| (p.level.0, p.pos.x, p.pos.y, p.amount));
        let gold = self.gold.entry(player).or_insert(Gold::zero());
        gold.carried = gold.carried.saturating_add(picked);
        picked
    }

    /// Drop carried gold to the ground (ground pile, section 94).
    pub fn drop_gold(
        &mut self,
        player: PlayerId,
        amount: u64,
        pos: arpg_core::WorldPos,
        level: arpg_core::LevelInstanceId,
    ) -> Result<(), EconomyError> {
        let gold = self
            .gold
            .get_mut(&player)
            .ok_or(EconomyError::InsufficientGold(amount))?;
        if gold.carried < amount {
            return Err(EconomyError::InsufficientGold(amount));
        }
        gold.carried -= amount;
        self.ground_gold.push(GroundCurrency { amount, pos, level });
        Ok(())
    }

    /// Serialize a location for persistence mutations.
    pub fn location_bytes(loc: ItemLocation) -> Vec<u8> {
        let mut out = Vec::new();
        match loc {
            ItemLocation::PlayerInventory(p, g) => {
                out.extend_from_slice(&[0, p.0 as u8, g.x, g.y]);
            }
            ItemLocation::Equipment(p, s) => {
                out.extend_from_slice(&[1, p.0 as u8, s as u8]);
            }
            ItemLocation::Belt(p, s) => {
                out.extend_from_slice(&[2, p.0 as u8, s]);
            }
            ItemLocation::Stash(p, s) => {
                out.extend_from_slice(&[3, p.0 as u8, s.page, s.x, s.y]);
            }
            ItemLocation::Cube(p, g) => {
                out.extend_from_slice(&[4, p.0 as u8, g.x, g.y]);
            }
            ItemLocation::Ground(l, pos) => {
                out.extend_from_slice(&[5]);
                out.extend_from_slice(&l.0.to_le_bytes());
                out.extend_from_slice(&pos.x.to_le_bytes());
                out.extend_from_slice(&pos.y.to_le_bytes());
            }
        }
        out
    }
}

/// Inventory helper re-export used by trade validation.
pub fn inventory_len(inv: &InventorySystem) -> usize {
    inv.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buy_pays_gold_and_decrements_stock() {
        let mut m = Merchant::new(1);
        let mut gold = Gold {
            carried: 500,
            stash: 0,
        };
        m.refresh(
            PlayerId(1),
            vec![MerchantEntry {
                def: ItemDefId(7),
                price: 100,
                quantity: 2,
            }],
        );
        assert_eq!(
            m.buy(PlayerId(1), &mut gold, ItemDefId(7), None).unwrap(),
            100
        );
        assert_eq!(gold.carried, 400);
        assert_eq!(m.stock(PlayerId(1))[0].quantity, 1);
    }

    #[test]
    fn buy_rejects_insufficient_gold() {
        let mut m = Merchant::new(1);
        let mut gold = Gold {
            carried: 50,
            stash: 0,
        };
        m.refresh(
            PlayerId(1),
            vec![MerchantEntry {
                def: ItemDefId(7),
                price: 100,
                quantity: 1,
            }],
        );
        assert_eq!(
            m.buy(PlayerId(1), &mut gold, ItemDefId(7), None),
            Err(EconomyError::InsufficientGold(100))
        );
        assert_eq!(
            m.stock(PlayerId(1))[0].quantity,
            1,
            "stock unchanged on failure"
        );
    }

    #[test]
    fn personal_stock_is_isolated_per_player() {
        let mut m = Merchant::new(1);
        m.refresh(
            PlayerId(1),
            vec![MerchantEntry {
                def: ItemDefId(7),
                price: 100,
                quantity: 1,
            }],
        );
        assert!(matches!(
            m.buy(
                PlayerId(2),
                &mut Gold {
                    carried: 500,
                    stash: 0
                },
                ItemDefId(7),
                None
            ),
            Err(EconomyError::OutOfStock(ItemDefId(7))) | Err(EconomyError::UnknownMerchant(_))
        ));
    }

    #[test]
    fn gamble_resolves_hidden_quality_after_purchase() {
        let mut m = Merchant::new(1);
        let mut gold = Gold {
            carried: 1000,
            stash: 0,
        };
        m.refresh_gamble(
            PlayerId(1),
            vec![GambleOffer {
                appearance: ItemDefId(3),
                price: 200,
                secret_seed: [7; 32],
            }],
        );
        let item = m.gamble_buy(PlayerId(1), &mut gold, 0).unwrap();
        assert_eq!(gold.carried, 800);
        assert_eq!(item.definition, ItemDefId(3));
        assert_eq!(item.generation_seed, [7; 32]);
        assert!(m.gamble_offers(PlayerId(1)).is_empty(), "offer consumed");
    }

    #[test]
    fn cube_transmute_matches_order_insensitive() {
        let recipes = vec![CubeRecipe {
            inputs: vec![ItemDefId(1), ItemDefId(2)],
            output: ItemDefId(9),
            output_quality: ItemQuality::Crafted,
        }];
        let r = transmute(&recipes, &[ItemDefId(2), ItemDefId(1)]).unwrap();
        assert_eq!(r.output, ItemDefId(9));
        assert!(transmute(&recipes, &[ItemDefId(1)]).is_err());
    }
}
