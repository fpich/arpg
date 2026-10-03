use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct WorldPos {
    pub x: i32,
    pub y: i32,
}

impl WorldPos {
    pub const ZERO: WorldPos = WorldPos { x: 0, y: 0 };

    pub fn new(x: i32, y: i32) -> WorldPos {
        WorldPos { x, y }
    }

    pub fn dist2(self, other: WorldPos) -> i64 {
        let dx = (self.x as i64) - (other.x as i64);
        let dy = (self.y as i64) - (other.y as i64);
        dx * dx + dy * dy
    }

    pub fn tile(self) -> (i32, i32) {
        (self.x / TILE_UNITS, self.y / TILE_UNITS)
    }
}

impl fmt::Display for WorldPos {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({}, {})", self.x, self.y)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct FixedVec2 {
    pub x: i32,
    pub y: i32,
}

impl FixedVec2 {
    pub const ZERO: FixedVec2 = FixedVec2 { x: 0, y: 0 };

    pub fn dot(self, other: FixedVec2) -> i64 {
        (self.x as i64) * (other.x as i64) + (self.y as i64) * (other.y as i64)
    }

    pub fn len2(self) -> i64 {
        self.dot(self)
    }
}

pub const TILE_UNITS: i32 = 256;
