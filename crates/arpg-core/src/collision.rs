use std::fmt;

/// Tile-based collision map (SPEC.md section 23). One bit per tile; a tile is
/// either walkable or blocking. Gameplay collision never depends on sprites.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollisionMap {
    pub width: u32,
    pub height: u32,
    /// true = walkable, false = blocking
    walkable: Vec<bool>,
}

impl CollisionMap {
    pub fn new(width: u32, height: u32) -> CollisionMap {
        CollisionMap {
            width,
            height,
            walkable: vec![true; (width * height) as usize],
        }
    }

    pub fn blocking(width: u32, height: u32) -> CollisionMap {
        CollisionMap {
            width,
            height,
            walkable: vec![false; (width * height) as usize],
        }
    }

    pub fn in_bounds(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && (x as u32) < self.width && (y as u32) < self.height
    }

    pub fn idx(&self, x: i32, y: i32) -> Option<usize> {
        if self.in_bounds(x, y) {
            Some(y as usize * self.width as usize + x as usize)
        } else {
            None
        }
    }

    pub fn is_walkable(&self, x: i32, y: i32) -> bool {
        self.idx(x, y).map(|i| self.walkable[i]).unwrap_or(false)
    }

    pub fn set_walkable(&mut self, x: i32, y: i32, walkable: bool) {
        if let Some(i) = self.idx(x, y) {
            self.walkable[i] = walkable;
        }
    }

    /// Walkability at world-unit resolution (1 tile = TILE_UNITS).
    pub fn walkable_at(&self, pos: WorldPos) -> bool {
        let (tx, ty) = pos.tile();
        self.is_walkable(tx, ty)
    }
}

use crate::world::WorldPos;

impl fmt::Display for CollisionMap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for y in 0..self.height {
            for x in 0..self.width {
                write!(
                    f,
                    "{}",
                    if self.walkable[(y * self.width + x) as usize] {
                        "."
                    } else {
                        "#"
                    }
                )?;
            }
            writeln!(f)?;
        }
        Ok(())
    }
}
