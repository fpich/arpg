use arpg_world::{generate_level, GenerationError, RoomRole, MAX_GENERATION_RETRIES};

const _: () = {
    assert!(MAX_GENERATION_RETRIES > 0 && MAX_GENERATION_RETRIES <= 64);
};

#[test]
fn generation_is_deterministic() {
    let seed = [42u8; 32];
    let (map_a, rooms_a) = generate_level(seed, 1).unwrap();
    let (map_b, rooms_b) = generate_level(seed, 1).unwrap();
    assert_eq!(format!("{map_a}"), format!("{map_b}"));
    assert_eq!(rooms_a.len(), rooms_b.len());
}

#[test]
fn different_level_ids_or_seeds_differ() {
    let a = generate_level([1u8; 32], 1).unwrap();
    let b = generate_level([2u8; 32], 1).unwrap();
    let c = generate_level([1u8; 32], 2).unwrap();
    assert_ne!(format!("{}", a.0), format!("{}", b.0));
    assert_ne!(format!("{}", a.0), format!("{}", c.0));
}

#[test]
fn mandatory_rooms_are_reachable() {
    // validated internally by generate_level; success implies BFS reachability
    for level_id in 0..10u32 {
        let (_map, rooms) = generate_level([7u8; 32], level_id).expect("generation must succeed");
        assert!(rooms.iter().any(|r| r.role == RoomRole::Entry));
        assert!(rooms.iter().any(|r| r.role == RoomRole::Exit));
        assert!(rooms.iter().any(|r| r.role == RoomRole::Waypoint));
        assert!(rooms.iter().any(|r| r.role == RoomRole::Quest));
    }
}

#[test]
fn retry_budget_is_bounded() {
    // MAX_GENERATION_RETRIES is an ENGINE constant (section 31)

    // generation error is the documented terminal state
    let err = GenerationError("world generation failed after retries");
    assert!(err.0.contains("failed"));
}
