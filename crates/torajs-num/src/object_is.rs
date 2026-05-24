//! `Object.is(a, b)` for f64 args — port of `runtime_str.c` L818.
//!
//! ES §7.2.10 SameValue: behaves like `===` except (i) NaN is the
//! same value as NaN, and (ii) +0 and -0 are different values.
//! The ±0 check is bit-level since IEEE 754 says `0.0 == -0.0`
//! evaluates true under FCmp.

/// `Object.is(a, b)` for two f64 arguments. Returns 1 on
/// SameValue-equal, 0 otherwise.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_object_is_f64(a: f64, b: f64) -> i64 {
    if a.is_nan() && b.is_nan() {
        return 1;
    }
    if a == 0.0 && b == 0.0 {
        // Bit-distinguish +0 vs -0.
        return if a.to_bits() == b.to_bits() { 1 } else { 0 };
    }
    if a == b { 1 } else { 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nan_equals_nan() {
        assert_eq!(unsafe { __torajs_object_is_f64(f64::NAN, f64::NAN) }, 1);
    }

    #[test]
    fn pos_zero_neg_zero_distinct() {
        assert_eq!(unsafe { __torajs_object_is_f64(0.0, -0.0) }, 0);
        assert_eq!(unsafe { __torajs_object_is_f64(0.0, 0.0) }, 1);
        assert_eq!(unsafe { __torajs_object_is_f64(-0.0, -0.0) }, 1);
    }

    #[test]
    fn ordinary_equal() {
        assert_eq!(unsafe { __torajs_object_is_f64(1.5, 1.5) }, 1);
        assert_eq!(unsafe { __torajs_object_is_f64(1.5, 2.0) }, 0);
    }
}
