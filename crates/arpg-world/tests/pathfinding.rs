use arpg_core::CollisionMap;
use arpg_world::a_star;

fn map_with_wall() -> CollisionMap {
    let mut m = CollisionMap::new(10, 10);
    // vertical wall with a gap at y=5
    for y in 0..10 {
        m.set_walkable(5, y, false);
    }
    m.set_walkable(5, 5, true);
    m
}

#[test]
fn astar_finds_path_around_wall() {
    let m = map_with_wall();
    let path = a_star(&m, (1, 1), (8, 1)).expect("path through the gap");
    assert_eq!(path.first(), Some(&(1, 1)));
    assert_eq!(path.last(), Some(&(8, 1)));
    // the path must pass through the gap
    assert!(path.contains(&(5, 5)));
    // every step is adjacent and walkable
    for w in path.windows(2) {
        let (x0, y0) = w[0];
        let (x1, y1) = w[1];
        assert!(
            (x0 - x1).abs() <= 1 && (y0 - y1).abs() <= 1,
            "adjacent steps"
        );
        assert!(m.is_walkable(x1, y1), "walkable steps");
    }
}

#[test]
fn astar_is_deterministic() {
    let m = map_with_wall();
    let a = a_star(&m, (1, 1), (8, 1)).unwrap();
    let b = a_star(&m, (1, 1), (8, 1)).unwrap();
    assert_eq!(a, b);
}

#[test]
fn astar_returns_none_when_unreachable() {
    let mut m = CollisionMap::new(10, 10);
    for y in 0..10 {
        m.set_walkable(5, y, false);
    } // fully sealed wall
    assert!(a_star(&m, (1, 1), (8, 1)).is_none());
}

#[test]
fn astar_no_corner_cutting() {
    // A diagonal step adjacent to a blocking corner must be forbidden:
    // (1,0) is blocked, so the diagonal (0,0)->(1,1) would cut the corner.
    // The route must go through (0,1) instead.
    let mut m = CollisionMap::new(3, 3);
    m.set_walkable(1, 0, false);
    let path = a_star(&m, (0, 0), (2, 2)).expect("a route exists around");
    assert_eq!(path[1], (0, 1), "must avoid the blocked corner");
    for w in path.windows(2) {
        let (x0, y0) = w[0];
        let (x1, y1) = w[1];
        if x0 != x1 && y0 != y1 {
            assert!(
                m.is_walkable(x0, y1) && m.is_walkable(x1, y0),
                "diagonal without corner cutting"
            );
        }
    }
}

#[test]
fn astar_trivial_cases() {
    let m = CollisionMap::new(5, 5);
    assert_eq!(a_star(&m, (2, 2), (2, 2)), Some(vec![(2, 2)]));
    assert!(a_star(&m, (0, 0), (4, 4)).is_some());
    assert!(a_star(&m, (-1, 0), (4, 4)).is_none(), "out of bounds start");
}
