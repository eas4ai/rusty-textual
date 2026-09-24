//! Named numeric conversions.
//!
//! The crate mixes several number types. Cell counts and indexes are
//! `usize`. Screen coordinates are `i32`, because a scrolled widget can start
//! above or left of the screen. Terminal sizes are `u16`. Animated offsets
//! and colour math use `f32` and `f64`.
//!
//! A bare `as` cast between these types can wrap, truncate or lose precision
//! with no trace at the call site. The [`Cast`] methods name the rule instead:
//!
//! - `to_<int>_sat` converts to an integer type and clamps the value to that
//!   type's range, so a negative value becomes 0 for an unsigned type. A float
//!   source is first rounded toward zero, and NaN becomes 0. These are the
//!   rules of a float `as` cast.
//! - `to_f32_lossy` and `to_f64_lossy` convert to a float and round to the
//!   nearest value it can hold. `f32` holds every integer up to 2^24 exactly,
//!   and `f64` every integer up to 2^53.
//!
//! A clamp is the wrong rule where a negative coordinate means "off screen".
//! Those call sites convert with `usize::try_from` and skip the value.

/// Numeric conversions with a named out-of-range rule. See the module docs.
pub(crate) trait Cast: Copy {
    fn to_u8_sat(self) -> u8;
    fn to_u16_sat(self) -> u16;
    fn to_u32_sat(self) -> u32;
    fn to_u64_sat(self) -> u64;
    fn to_usize_sat(self) -> usize;
    fn to_i32_sat(self) -> i32;
    fn to_i64_sat(self) -> i64;
    fn to_isize_sat(self) -> isize;
    fn to_f32_lossy(self) -> f32;
    fn to_f64_lossy(self) -> f64;
}

/// Saturating integer conversion: the value, or the nearer bound of `$dst`.
macro_rules! sat_int {
    (unsigned $v:expr, $dst:ty) => {
        <$dst>::try_from($v).unwrap_or(<$dst>::MAX)
    };
    (signed $v:expr, $dst:ty) => {
        <$dst>::try_from($v).unwrap_or(if $v < 0 { <$dst>::MIN } else { <$dst>::MAX })
    };
}

macro_rules! impl_cast_int {
    ($sign:ident: $($src:ty),*) => {$(
        // std has `From` for only some integer-to-float pairs. `as` rounds
        // to nearest, which is the documented `_lossy` rule.
        #[allow(clippy::cast_precision_loss, clippy::cast_lossless)]
        impl Cast for $src {
            fn to_u8_sat(self) -> u8 { sat_int!($sign self, u8) }
            fn to_u16_sat(self) -> u16 { sat_int!($sign self, u16) }
            fn to_u32_sat(self) -> u32 { sat_int!($sign self, u32) }
            fn to_u64_sat(self) -> u64 { sat_int!($sign self, u64) }
            fn to_usize_sat(self) -> usize { sat_int!($sign self, usize) }
            fn to_i32_sat(self) -> i32 { sat_int!($sign self, i32) }
            fn to_i64_sat(self) -> i64 { sat_int!($sign self, i64) }
            fn to_isize_sat(self) -> isize { sat_int!($sign self, isize) }
            fn to_f32_lossy(self) -> f32 { self as f32 }
            fn to_f64_lossy(self) -> f64 { self as f64 }
        }
    )*};
}

impl_cast_int!(unsigned: u32, u64, u128, usize);
impl_cast_int!(signed: i32, i64, isize);

macro_rules! float_to_int_methods {
    () => {
        fn to_u8_sat(self) -> u8 {
            self as u8
        }
        fn to_u16_sat(self) -> u16 {
            self as u16
        }
        fn to_u32_sat(self) -> u32 {
            self as u32
        }
        fn to_u64_sat(self) -> u64 {
            self as u64
        }
        fn to_usize_sat(self) -> usize {
            self as usize
        }
        fn to_i32_sat(self) -> i32 {
            self as i32
        }
        fn to_i64_sat(self) -> i64 {
            self as i64
        }
        fn to_isize_sat(self) -> isize {
            self as isize
        }
    };
}

// A float `as` cast to an integer saturates, rounds toward zero and maps NaN
// to 0: exactly the documented `_sat` rule.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
impl Cast for f32 {
    float_to_int_methods!();
    fn to_f32_lossy(self) -> f32 {
        self
    }
    fn to_f64_lossy(self) -> f64 {
        f64::from(self)
    }
}

// As for `f32`; `f64` to `f32` rounds to nearest, the documented `_lossy`
// rule.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
impl Cast for f64 {
    float_to_int_methods!();
    fn to_f32_lossy(self) -> f32 {
        self as f32
    }
    fn to_f64_lossy(self) -> f64 {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::Cast;

    #[test]
    fn unsigned_to_narrower_int_saturates_at_max() {
        assert_eq!(70_000_usize.to_u16_sat(), u16::MAX);
        assert_eq!(usize::MAX.to_i32_sat(), i32::MAX);
        assert_eq!(usize::MAX.to_isize_sat(), isize::MAX);
        assert_eq!(u64::MAX.to_i64_sat(), i64::MAX);
        assert_eq!(u128::MAX.to_u64_sat(), u64::MAX);
        assert_eq!(300_u32.to_u8_sat(), u8::MAX);
    }

    #[test]
    fn in_range_values_are_unchanged() {
        assert_eq!(80_usize.to_u16_sat(), 80);
        assert_eq!(80_usize.to_i32_sat(), 80);
        assert_eq!((-7_i32).to_isize_sat(), -7);
        assert_eq!(42_i64.to_usize_sat(), 42);
        assert_eq!(9.99_f32.to_u8_sat(), 9);
        assert_eq!((-3.7_f64).to_i32_sat(), -3);
    }

    #[test]
    fn negative_to_unsigned_saturates_at_zero() {
        assert_eq!((-1_i32).to_usize_sat(), 0);
        assert_eq!(isize::MIN.to_usize_sat(), 0);
        assert_eq!((-5_i64).to_u16_sat(), 0);
        assert_eq!((-0.5_f32).to_usize_sat(), 0);
        assert_eq!((-300.0_f64).to_u8_sat(), 0);
    }

    #[test]
    fn signed_to_narrower_signed_saturates_at_both_bounds() {
        assert_eq!(i64::MIN.to_i32_sat(), i32::MIN);
        assert_eq!(i64::MAX.to_i32_sat(), i32::MAX);
        assert_eq!(isize::MIN.to_i32_sat(), i32::MIN);
    }

    #[test]
    fn float_to_int_saturates_and_maps_nan_to_zero() {
        assert_eq!(1e9_f32.to_u16_sat(), u16::MAX);
        assert_eq!(f32::INFINITY.to_usize_sat(), usize::MAX);
        assert_eq!(f64::NEG_INFINITY.to_i64_sat(), i64::MIN);
        assert_eq!(f32::NAN.to_u8_sat(), 0);
        assert_eq!(f64::NAN.to_i32_sat(), 0);
    }

    // Float results are compared by bit pattern: these tests check exact
    // values, not closeness.
    #[test]
    fn int_to_float_is_exact_in_range_and_rounds_beyond() {
        let two_pow_24 = 16_777_216.0_f32.to_bits();
        assert_eq!(16_777_216_usize.to_f32_lossy().to_bits(), two_pow_24);
        // 2^24 + 1 has no f32 form; it rounds to the nearest even, 2^24.
        assert_eq!(16_777_217_usize.to_f32_lossy().to_bits(), two_pow_24);
        assert_eq!(
            9_007_199_254_740_992_u64.to_f64_lossy().to_bits(),
            9_007_199_254_740_992.0_f64.to_bits()
        );
        assert_eq!((-12_i32).to_f32_lossy().to_bits(), (-12.0_f32).to_bits());
    }

    #[test]
    fn float_to_float_rounds_to_nearest() {
        assert_eq!(0.1_f64.to_f32_lossy().to_bits(), 0.1_f32.to_bits());
        assert_eq!(1e300_f64.to_f32_lossy().to_bits(), f32::INFINITY.to_bits());
        assert_eq!(
            0.1_f32.to_f64_lossy().to_bits(),
            f64::from(0.1_f32).to_bits()
        );
    }
}
