use arpg_core::{Fixed, RoundingMode};

#[test]
fn fixed_mul_div_roundtrip() {
    let a = Fixed::from_ticks(3);
    let b = Fixed::from_ticks(7);
    assert_eq!(a * b, Fixed::from_ticks(21));
    assert_eq!(
        (a * b).div_fp(b, RoundingMode::TowardZero),
        a,
        "division by the same factor must round-trip exactly"
    );
}

#[test]
fn rounding_modes_differ_predictably() {
    let a = Fixed::from_ticks(7);
    let b = Fixed::from_ticks(3);
    let tz = a.div_fp(b, RoundingMode::TowardZero);
    let fl = a.div_fp(b, RoundingMode::Floor);
    let ce = a.div_fp(b, RoundingMode::Ceil);
    let ne = a.div_fp(b, RoundingMode::Nearest);
    // 7/3 = 2.333 ticks = 597.33 fixed units
    assert_eq!(tz, Fixed(597));
    assert_eq!(fl, Fixed(597));
    assert_eq!(ce, Fixed(598));
    assert_eq!(ne, Fixed(597));
}

#[test]
fn rounding_negative_values() {
    let a = Fixed::from_ticks(-7);
    let b = Fixed::from_ticks(3);
    // -7/3 = -2.333 ticks = -597.33 fixed units
    assert_eq!(a.div_fp(b, RoundingMode::TowardZero), Fixed(-597));
    assert_eq!(a.div_fp(b, RoundingMode::Floor), Fixed(-598));
    assert_eq!(a.div_fp(b, RoundingMode::Ceil), Fixed(-597));
    assert_eq!(a.div_fp(b, RoundingMode::Nearest), Fixed(-597));
}

#[test]
fn division_by_zero_is_not_representable() {
    let a = Fixed::from_ticks(1);
    let z = Fixed::ZERO;
    let result = std::panic::catch_unwind(move || a.div_fp(z, RoundingMode::Floor));
    assert!(
        result.is_err(),
        "div by zero must panic (impossible invariant), not wrap"
    );
}
