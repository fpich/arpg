use arpg_core::CollisionMap;
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

/// Deterministic A* (SPEC.md section 27).
/// Fixed neighbor order: N, NE, E, SE, S, SW, W, NW.
/// Tie-break: f_score, then h_score, then node_index.
const NEIGHBOR_ORDER: [(i32, i32); 8] = [
    (0, -1),  // N
    (1, -1),  // NE
    (1, 0),   // E
    (1, 1),   // SE
    (0, 1),   // S
    (-1, 1),  // SW
    (-1, 0),  // W
    (-1, -1), // NW
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Node {
    f: u32,
    h: u32,
    index: u32,
    x: i32,
    y: i32,
}

impl Ord for Node {
    fn cmp(&self, other: &Self) -> Ordering {
        // BinaryHeap is a max-heap: reverse for min-heap semantics with
        // the canonical tie-break order (f, h, node_index).
        other
            .f
            .cmp(&self.f)
            .then(other.h.cmp(&self.h))
            .then(other.index.cmp(&self.index))
    }
}

impl PartialOrd for Node {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[inline]
fn octile(dx: i32, dy: i32) -> u32 {
    let dx = dx.unsigned_abs();
    let dy = dy.unsigned_abs();
    let (mi, ma) = if dx < dy { (dx, dy) } else { (dy, dx) };
    // octile: ma - mi + sqrt(2)*mi ~= (ma - mi) + 141/100 * mi, in integer units
    (ma - mi) + mi * 141 / 100
}

/// Returns the tile path from start to goal (inclusive), or None if the goal
/// is unreachable. Diagonal moves require both adjacent orthogonal tiles to
/// be walkable (no corner cutting).
pub fn a_star(map: &CollisionMap, start: (i32, i32), goal: (i32, i32)) -> Option<Vec<(i32, i32)>> {
    if !map.is_walkable(start.0, start.1) || !map.is_walkable(goal.0, goal.1) {
        return None;
    }
    if start == goal {
        return Some(vec![start]);
    }

    let mut next_index: u32 = 0;
    let start_node = Node {
        f: octile(goal.0 - start.0, goal.1 - start.1),
        h: octile(goal.0 - start.0, goal.1 - start.1),
        index: 0,
        x: start.0,
        y: start.1,
    };
    next_index += 1;

    let mut open: BinaryHeap<Node> = BinaryHeap::new();
    open.push(start_node);
    let mut came_from: HashMap<(i32, i32), (i32, i32)> = HashMap::new();
    let mut g_score: HashMap<(i32, i32), u32> = HashMap::new();
    g_score.insert(start, 0);

    while let Some(current) = open.pop() {
        if current.x == goal.0 && current.y == goal.1 {
            let mut path = vec![goal];
            let mut cur = goal;
            while let Some(&prev) = came_from.get(&cur) {
                path.push(prev);
                cur = prev;
            }
            path.reverse();
            return Some(path);
        }
        for (dx, dy) in NEIGHBOR_ORDER {
            let nx = current.x + dx;
            let ny = current.y + dy;
            if !map.is_walkable(nx, ny) {
                continue;
            }
            if dx != 0 && dy != 0 {
                // no corner cutting
                if !map.is_walkable(current.x + dx, current.y)
                    || !map.is_walkable(current.x, current.y + dy)
                {
                    continue;
                }
            }
            let cost = if dx != 0 && dy != 0 { 141 } else { 100 };
            let tentative_g = g_score.get(&(current.x, current.y)).copied().unwrap_or(0) + cost;
            let better = match g_score.get(&(nx, ny)) {
                Some(&g) => tentative_g < g,
                None => true,
            };
            if better {
                g_score.insert((nx, ny), tentative_g);
                came_from.insert((nx, ny), (current.x, current.y));
                let h = octile(goal.0 - nx, goal.1 - ny);
                open.push(Node {
                    f: tentative_g + h,
                    h,
                    index: next_index,
                    x: nx,
                    y: ny,
                });
                next_index += 1;
            }
        }
    }
    None
}
