use arpg_core::EntityId;
use arpg_core::WorldPos;
use std::collections::BTreeMap;

pub const GRID_CELL_SIZE: i32 = 8 * arpg_core::TILE_UNITS; // 8 tiles

/// Uniform spatial grid (SPEC.md section 24). Cells store EntityIds;
/// queries return candidates that must then be verified by distance.
#[derive(Debug, Default)]
pub struct SpatialGrid {
    cell_size: i32,
    cells: BTreeMap<(i32, i32), Vec<EntityId>>,
}

impl SpatialGrid {
    pub fn new(cell_size: i32) -> SpatialGrid {
        SpatialGrid {
            cell_size: cell_size.max(1),
            cells: BTreeMap::new(),
        }
    }

    fn cell_of(&self, pos: WorldPos) -> (i32, i32) {
        (
            pos.x.div_euclid(self.cell_size),
            pos.y.div_euclid(self.cell_size),
        )
    }

    pub fn insert(&mut self, entity: EntityId, pos: WorldPos) {
        self.cells
            .entry(self.cell_of(pos))
            .or_default()
            .push(entity);
    }

    pub fn remove(&mut self, entity: EntityId, pos: WorldPos) {
        let key = self.cell_of(pos);
        if let Some(v) = self.cells.get_mut(&key) {
            v.retain(|&e| e != entity);
        }
    }

    pub fn update(&mut self, entity: EntityId, from: WorldPos, to: WorldPos) {
        if self.cell_of(from) != self.cell_of(to) {
            self.remove(entity, from);
            self.insert(entity, to);
        }
    }

    /// All entities in the cells overlapping the query radius. The result is
    /// canonical (sorted by EntityId) before any gameplay selection
    /// (SPEC.md section 24).
    pub fn query_radius(&self, pos: WorldPos, radius: i32) -> Vec<EntityId> {
        let mut out = Vec::new();
        let r = radius.div_euclid(self.cell_size) + 1;
        let (cx, cy) = self.cell_of(pos);
        for y in cy - r..=cy + r {
            for x in cx - r..=cx + r {
                if let Some(ids) = self.cells.get(&(x, y)) {
                    out.extend_from_slice(ids);
                }
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }
}
