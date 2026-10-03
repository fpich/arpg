use std::fmt;
use std::ops::{Add, Mul, Neg, Sub};

/// Fixed-point gameplay quantity in 1/256 sub-tile units (Q24.8-like, i32 backing).
/// No f32/f64 is ever used for a deterministic gameplay calculation (INV-006).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[repr(transparent)]
pub struct Fixed(pub i32);

pub const FIXED_ONE: i32 = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoundingMode {
    Floor,
    Ceil,
    TowardZero,
    Nearest,
}

impl Fixed {
    pub const ZERO: Fixed = Fixed(0);
    pub const ONE: Fixed = Fixed(FIXED_ONE);

    pub fn from_ticks(n: i64) -> Fixed {
        Fixed((n * FIXED_ONE as i64) as i32)
    }

    pub fn mul_fp(self, other: Fixed) -> Fixed {
        Fixed(((self.0 as i64) * (other.0 as i64) / FIXED_ONE as i64) as i32)
    }

    /// Division with an explicit, formula-declared rounding rule (SPEC.md section 45).
    pub fn div_fp(self, other: Fixed, mode: RoundingMode) -> Fixed {
        let num = (self.0 as i64) * FIXED_ONE as i64;
        let den = other.0 as i64;
        match mode {
            RoundingMode::TowardZero => Fixed((num / den) as i32),
            RoundingMode::Floor => Fixed(num.div_euclid(den) as i32),
            RoundingMode::Ceil => Fixed((-num).div_euclid(-den) as i32),
            RoundingMode::Nearest => {
                let q = num.div_euclid(den);
                let r = num.rem_euclid(den);
                if 2 * r >= den.abs() {
                    Fixed((if den < 0 { q - 1 } else { q + 1 }) as i32)
                } else {
                    Fixed(q as i32)
                }
            }
        }
    }
}

impl Add for Fixed {
    type Output = Fixed;
    fn add(self, rhs: Fixed) -> Fixed {
        Fixed(self.0.checked_add(rhs.0).expect("fixed overflow"))
    }
}

impl Sub for Fixed {
    type Output = Fixed;
    fn sub(self, rhs: Fixed) -> Fixed {
        Fixed(self.0.checked_sub(rhs.0).expect("fixed overflow"))
    }
}

impl Mul for Fixed {
    type Output = Fixed;
    fn mul(self, rhs: Fixed) -> Fixed {
        self.mul_fp(rhs)
    }
}

impl Neg for Fixed {
    type Output = Fixed;
    fn neg(self) -> Fixed {
        Fixed(self.0.checked_neg().expect("fixed overflow"))
    }
}

impl fmt::Display for Fixed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let int = self.0 / FIXED_ONE;
        let frac = self.0.rem_euclid(FIXED_ONE);
        write!(f, "{}.{:03}", int, frac * 1000 / FIXED_ONE)
    }
}
