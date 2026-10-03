use arpg_core::{EntityId, LevelDefId, RngDomain, RngStreams};
use rand_chacha::rand_core::RngCore;

const ROOT: [u8; 32] = [7u8; 32];

#[test]
fn rng_vectors_are_stable() {
    let streams = RngStreams::new(ROOT);
    let mut rng = streams.derive(RngDomain::World(LevelDefId(3)), 42);
    let mut words = [0u32; 4];
    for w in words.iter_mut() {
        *w = rng.next_u32();
    }
    let expected = [224886117u32, 100607545, 827060249, 1231111509];
    assert_eq!(
        words, expected,
        "RNG vectors changed: update SPEC.md section 15"
    );
}

#[test]
fn independent_domains_do_not_interfere() {
    let streams = RngStreams::new(ROOT);
    let mut a = streams.derive(RngDomain::Ai(EntityId(1)), 0);
    let first = a.next_u64();

    let mut b = streams.derive(RngDomain::Ai(EntityId(2)), 0);
    let second = b.next_u64();

    let mut c = streams.derive(RngDomain::Ai(EntityId(1)), 0);
    let replay = c.next_u64();

    assert_eq!(first, replay);
    assert_ne!(first, second, "different stable ids must diverge");
}
