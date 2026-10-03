use crate::id::{EntityId, LevelDefId};
use blake3::Hasher;
use rand_chacha::rand_core::{RngCore, SeedableRng};
use rand_chacha::ChaCha8Rng;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RngDomain {
    World(LevelDefId),
    Spawn(LevelDefId),
    Ai(EntityId),
    Attack(EntityId),
    Missile(EntityId),
    Drop(EntityId),
    TreasureClass(EntityId),
    Quality(EntityId),
    Affixes(EntityId),
    Sockets(EntityId),
    Merchant(u64),
}

impl RngDomain {
    fn tag(self) -> &'static str {
        match self {
            RngDomain::World(_) => "world",
            RngDomain::Spawn(_) => "spawn",
            RngDomain::Ai(_) => "ai",
            RngDomain::Attack(_) => "attack",
            RngDomain::Missile(_) => "missile",
            RngDomain::Drop(_) => "drop",
            RngDomain::TreasureClass(_) => "tc",
            RngDomain::Quality(_) => "quality",
            RngDomain::Affixes(_) => "affixes",
            RngDomain::Sockets(_) => "sockets",
            RngDomain::Merchant(_) => "merchant",
        }
    }

    fn stable_ids(self) -> [u64; 2] {
        match self {
            RngDomain::World(id) | RngDomain::Spawn(id) => [id.0 as u64, 0],
            RngDomain::Ai(id)
            | RngDomain::Attack(id)
            | RngDomain::Missile(id)
            | RngDomain::Drop(id)
            | RngDomain::TreasureClass(id)
            | RngDomain::Quality(id)
            | RngDomain::Affixes(id)
            | RngDomain::Sockets(id) => [id.0, 0],
            RngDomain::Merchant(npc) => [npc, 0],
        }
    }
}

pub struct RngStreams {
    root_seed: [u8; 32],
}

impl RngStreams {
    pub fn new(root_seed: [u8; 32]) -> RngStreams {
        RngStreams { root_seed }
    }

    pub fn derive(&self, domain: RngDomain, sequence: u64) -> ChaCha8Rng {
        let mut hasher = Hasher::new();
        hasher.update(&self.root_seed);
        hasher.update(domain.tag().as_bytes());
        hasher.update(&domain.stable_ids()[0].to_le_bytes());
        hasher.update(&sequence.to_le_bytes());
        let seed = *hasher.finalize().as_bytes();
        ChaCha8Rng::from_seed(seed)
    }
}

pub fn next_u32_bounded(rng: &mut ChaCha8Rng, bound_exclusive: u32) -> u32 {
    rng.next_u32() % bound_exclusive.max(1)
}
