use arpg_core::{
    CollisionMap, InteractableKind, ObjectDefId, ObjectId, ObjectInstance, WorldPos, TILE_UNITS,
};

pub const MAX_GENERATION_RETRIES: u32 = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoomRole {
    Entry,
    Mandatory,
    Quest,
    Waypoint,
    Exit,
    OptionalBranch,
}

/// Logical level graph (SPEC.md section 29.1).
#[derive(Debug, Clone)]
pub struct LevelGraph {
    pub rooms: Vec<GraphRoom>,
}

#[derive(Debug, Clone)]
pub struct GraphRoom {
    pub id: u32,
    pub role: RoomRole,
    pub connections: Vec<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn intersects(&self, other: &Rect) -> bool {
        self.x < other.x + other.w
            && other.x < self.x + self.w
            && self.y < other.y + other.h
            && other.y < self.y + self.h
    }

    pub fn center(&self) -> (i32, i32) {
        (self.x + self.w / 2, self.y + self.h / 2)
    }
}

#[derive(Debug, Clone)]
pub struct RoomInstance {
    pub id: u32,
    pub role: RoomRole,
    pub rect: Rect,
}

#[derive(Debug, Clone)]
pub struct GenerationError(pub &'static str);

/// Deterministic level generation (SPEC.md sections 29-31).
/// Two steps: logical graph, then spatial materialization. A BFS validation
/// guarantees reachability of mandatory nodes; invalid generations retry
/// with a derived sub-seed without consuming the main seed arbitrarily.
pub fn generate_level(
    seed: [u8; 32],
    level_id: u32,
) -> Result<(CollisionMap, Vec<RoomInstance>, Vec<ObjectInstance>), GenerationError> {
    let graph = build_graph(&seed, level_id);
    for retry in 0..MAX_GENERATION_RETRIES {
        let sub_seed = derive_sub_seed(&seed, level_id, retry);
        if let Some(result) = materialize(&graph, &sub_seed) {
            let (map, rooms) = result;
            if validate(&map, &rooms) {
                let objects = place_objects(&rooms, &sub_seed);
                return Ok((map, rooms, objects));
            }
        }
    }
    Err(GenerationError("world generation failed after retries"))
}

/// Deterministic interactive object placement (SPEC.md section 98): one
/// chest per mandatory room, one shrine in the quest room, one waypoint in
/// the waypoint room, one barrel in the optional branch.
fn place_objects(rooms: &[RoomInstance], sub_seed: &[u8; 32]) -> Vec<ObjectInstance> {
    let mut objects = Vec::new();
    let mut next_id = 1u64;
    let def = |n: u32| ObjectDefId(n);
    let mut place = |kind: InteractableKind, defn: u32, room: &Rect| {
        let (cx, cy) = room.center();
        let pos = WorldPos::new(
            cx * TILE_UNITS + (sub_seed[next_id as usize % 32] as i32 - 128),
            cy * TILE_UNITS,
        );
        let id = next_id;
        next_id += 1;
        ObjectInstance::new(ObjectId(id), def(defn), kind, pos, arpg_core::Tick(0))
    };
    for room in rooms {
        match room.role {
            RoomRole::Mandatory => {
                objects.push(place(InteractableKind::Chest, 1, &room.rect));
            }
            RoomRole::Quest => {
                objects.push(place(InteractableKind::Shrine, 2, &room.rect));
            }
            RoomRole::Waypoint => {
                objects.push(place(InteractableKind::Waypoint, 3, &room.rect));
            }
            RoomRole::OptionalBranch => {
                objects.push(place(InteractableKind::Barrel, 4, &room.rect));
            }
            _ => {}
        }
    }
    objects
}

fn build_graph(seed: &[u8; 32], level_id: u32) -> LevelGraph {
    // Deterministic logical layout: entry -> mandatory chain with an optional
    // branch, waypoint midway, quest room before the exit.
    let branch = (seed[0].wrapping_add(level_id as u8)) % 2 == 0;
    let mut rooms = vec![
        GraphRoom {
            id: 0,
            role: RoomRole::Entry,
            connections: vec![1],
        },
        GraphRoom {
            id: 1,
            role: RoomRole::Mandatory,
            connections: vec![0, 2],
        },
        GraphRoom {
            id: 2,
            role: RoomRole::Waypoint,
            connections: vec![1, 3],
        },
        GraphRoom {
            id: 3,
            role: RoomRole::Quest,
            connections: vec![2, 4],
        },
        GraphRoom {
            id: 4,
            role: RoomRole::Exit,
            connections: vec![3],
        },
    ];
    if branch {
        rooms[1].connections.push(5);
        rooms.push(GraphRoom {
            id: 5,
            role: RoomRole::OptionalBranch,
            connections: vec![1],
        });
    }
    LevelGraph { rooms }
}

fn derive_sub_seed(seed: &[u8; 32], level_id: u32, retry: u32) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(seed);
    hasher.update(&level_id.to_le_bytes());
    hasher.update(&retry.to_le_bytes());
    *hasher.finalize().as_bytes()
}

fn materialize(
    graph: &LevelGraph,
    sub_seed: &[u8; 32],
) -> Option<(CollisionMap, Vec<RoomInstance>)> {
    // Simple linear corridor materialization with jittered room sizes.
    let room_w = 8 + (sub_seed[0] % 4) as i32;
    let room_h = 6 + (sub_seed[1] % 3) as i32;
    let corridor_len = 2 + (sub_seed[2] % 2) as i32;
    let map_w = (room_w + corridor_len) * graph.rooms.len() as i32 + 2;
    let map_h = room_h + 4;

    let mut map = CollisionMap::blocking(map_w as u32, map_h as u32);
    let mut rooms = Vec::new();
    let mut x = 1;
    for room in &graph.rooms {
        let y = 2;
        for dy in 0..room_h {
            for dx in 0..room_w {
                map.set_walkable(x + dx, y + dy, true);
            }
        }
        rooms.push(RoomInstance {
            id: room.id,
            role: room.role,
            rect: Rect {
                x,
                y,
                w: room_w,
                h: room_h,
            },
        });
        // carve corridor to the next room (along room 1's connection chain)
        if let Some(&next) = room.connections.iter().filter(|&&c| c > room.id).max() {
            let nx = x + room_w;
            for dx in 0..=corridor_len + (room_w * (next - room.id - 1) as i32) {
                let cx = nx + dx;
                if cx < map_w - 1 {
                    map.set_walkable(cx, y + room_h / 2, true);
                }
            }
        }
        x += room_w + corridor_len;
    }
    Some((map, rooms))
}

/// Structural validation (SPEC.md section 30): BFS over the collision map.
/// Entry, waypoint, quest and exit must be mutually reachable.
fn validate(map: &CollisionMap, rooms: &[RoomInstance]) -> bool {
    let mandatory: Vec<&RoomInstance> = rooms
        .iter()
        .filter(|r| {
            matches!(
                r.role,
                RoomRole::Entry | RoomRole::Waypoint | RoomRole::Quest | RoomRole::Exit
            )
        })
        .collect();
    if mandatory.is_empty() {
        return false;
    }
    let entry = mandatory
        .iter()
        .find(|r| r.role == RoomRole::Entry)
        .expect("entry present");
    let entry_center = entry.rect.center();
    for room in &mandatory {
        let center = room.rect.center();
        if crate::astar::a_star(map, entry_center, center).is_none() {
            return false;
        }
    }
    true
}
