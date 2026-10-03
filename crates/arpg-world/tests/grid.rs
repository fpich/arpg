use arpg_core::{EntityId, WorldPos};
use arpg_world::SpatialGrid;

#[test]
fn query_returns_canonical_sorted_ids() {
    let mut grid = SpatialGrid::new(2048);
    // insert in reverse order: query must sort by EntityId
    for i in (1..=5).rev() {
        grid.insert(EntityId(i), WorldPos::new((i * 100) as i32, 0));
    }
    let found = grid.query_radius(WorldPos::new(300, 0), 256);
    assert_eq!(
        found,
        vec![
            EntityId(1),
            EntityId(2),
            EntityId(3),
            EntityId(4),
            EntityId(5)
        ]
    );
}

#[test]
fn update_moves_entity_between_cells() {
    let mut grid = SpatialGrid::new(256); // 1 tile cells
    let e = EntityId(1);
    let a = WorldPos::new(10, 10);
    let b = WorldPos::new(1000, 10);
    grid.insert(e, a);
    grid.update(e, a, b);
    assert!(grid.query_radius(a, 100).is_empty());
    assert_eq!(grid.query_radius(b, 100), vec![e]);
}

#[test]
fn query_radius_respects_distance_cells() {
    let mut grid = SpatialGrid::new(2048);
    grid.insert(EntityId(1), WorldPos::new(0, 0));
    grid.insert(EntityId(2), WorldPos::new(10_000, 10_000));
    let near = grid.query_radius(WorldPos::new(0, 0), 256);
    assert_eq!(near, vec![EntityId(1)]);
}
