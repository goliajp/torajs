//! Number predicates — `Number.isNaN` / `Number.isFinite` /
//! `Number.isInteger` / `Number.isSafeInteger`.
//!
//! Each predicate has two flavors — `_f` (f64 receiver) and `_i`
//! (i64 receiver) — because the SSA layer types numbers as the
//! tighter `i64` whenever it can prove an integer (no fractional
//! literal / division / Math op). The `_i` variants are typically
//! trivial (e.g. `is_integer_i` is always `true`); the `_f`
//! variants do the real spec check via `f64` introspection.
//!
//! Returns `i64` (0/1) not `bool` to match the IR-side ABI. Other
//! `*_from`-style fns in the workspace use the same convention so
//! ssa_lower doesn't need a special-case truncation.
//!
//! Port of `runtime_str.c` lines 4073-4112 (P3.2-c.1, 2026-05-23).

const MAX_SAFE_INTEGER_F: f64 = 9_007_199_254_740_991.0; // 2^53 - 1
const MAX_SAFE_INTEGER_I: i64 = 9_007_199_254_740_991;

// ============================================================
// Number.isNaN
// ============================================================

/// `Number.isNaN(n)` for f64 receivers. `true` iff n is NaN.
/// Distinct from global `isNaN(value)` which coerces non-numbers;
/// `Number.isNaN` does NOT coerce.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_num_is_nan_f(n: f64) -> i64 {
    if n.is_nan() { 1 } else { 0 }
}

/// i64 variant — i64 is never NaN, so always `false`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_num_is_nan_i(_n: i64) -> i64 {
    0
}

// ============================================================
// Number.isFinite
// ============================================================

/// `Number.isFinite(n)` for f64 receivers. `true` iff n is finite
/// (not NaN, not ±Infinity).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_num_is_finite_f(n: f64) -> i64 {
    if n.is_finite() { 1 } else { 0 }
}

/// i64 variant — i64 is always finite.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_num_is_finite_i(_n: i64) -> i64 {
    1
}

// ============================================================
// Number.isInteger
// ============================================================

/// `Number.isInteger(n)` for f64 receivers. `true` iff n is finite
/// AND has no fractional part. ECMA-262 §20.1.2.3.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_num_is_integer_f(n: f64) -> i64 {
    if n.is_finite() && n.floor() == n {
        1
    } else {
        0
    }
}

/// i64 variant — i64 is always an integer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_num_is_integer_i(_n: i64) -> i64 {
    1
}

// ============================================================
// Number.isSafeInteger
// ============================================================

/// `Number.isSafeInteger(n)` for f64 receivers. `true` iff n is an
/// integer-valued number within `[-(2^53 - 1), 2^53 - 1]`. "Safe"
/// means a round-trip through f64 preserves the value exactly.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_num_is_safe_integer_f(n: f64) -> i64 {
    if !n.is_finite() {
        return 0;
    }
    if n.floor() != n {
        return 0;
    }
    if n >= -MAX_SAFE_INTEGER_F && n <= MAX_SAFE_INTEGER_F {
        1
    } else {
        0
    }
}

/// i64 variant — i64 is always an integer; the only check is range
/// against [-(2^53 - 1), 2^53 - 1]. i64 outside that range still
/// "is an integer" but isn't representable lossless in f64.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_num_is_safe_integer_i(n: i64) -> i64 {
    if n >= -MAX_SAFE_INTEGER_I && n <= MAX_SAFE_INTEGER_I {
        1
    } else {
        0
    }
}

// ============================================================
// Number.is{NaN,Finite,Integer,SafeInteger}(Any) — tag-dispatch
// ============================================================
//
// S341 — Pre-fix `Number.is*(Any)` short-circuited to `false` in
// ssa_lower regardless of the Any-box tag (silent-wrong vs spec
// §21.1.2.{3,4,5,7}: spec is strict-Type, but Any can hold a
// Number at runtime → must dispatch on the unboxed tag).
//
// Tag layout (see crates/torajs-anyvalue/src/nanbox.rs):
//   ANY_NULL=0, ANY_BOOL=1, ANY_I64=2, ANY_F64=3, ANY_HEAP=4,
//   ANY_UNDEF=5. Only tag ∈ {2, 3} is a Number — every other
//   tag returns 0 (false) per spec strict-Type-check.

const ANY_I64: i64 = 2;
const ANY_F64: i64 = 3;

/// `Number.isNaN(any)` — tag-dispatch on the AnyValue (tag, val) pair.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_num_is_nan_any(tag: i64, val: i64) -> i64 {
    match tag {
        ANY_F64 => unsafe { __torajs_num_is_nan_f(f64::from_bits(val as u64)) },
        _ => 0,
    }
}

/// `Number.isFinite(any)` — tag-dispatch on the AnyValue (tag, val) pair.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_num_is_finite_any(tag: i64, val: i64) -> i64 {
    match tag {
        ANY_I64 => 1,
        ANY_F64 => unsafe { __torajs_num_is_finite_f(f64::from_bits(val as u64)) },
        _ => 0,
    }
}

/// `Number.isInteger(any)` — tag-dispatch on the AnyValue (tag, val) pair.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_num_is_integer_any(tag: i64, val: i64) -> i64 {
    match tag {
        ANY_I64 => 1,
        ANY_F64 => unsafe { __torajs_num_is_integer_f(f64::from_bits(val as u64)) },
        _ => 0,
    }
}

/// `Number.isSafeInteger(any)` — tag-dispatch on the AnyValue (tag, val) pair.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_num_is_safe_integer_any(tag: i64, val: i64) -> i64 {
    match tag {
        ANY_I64 => unsafe { __torajs_num_is_safe_integer_i(val) },
        ANY_F64 => unsafe { __torajs_num_is_safe_integer_f(f64::from_bits(val as u64)) },
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_nan_distinguishes_only_nan() {
        assert_eq!(unsafe { __torajs_num_is_nan_f(f64::NAN) }, 1);
        assert_eq!(unsafe { __torajs_num_is_nan_f(0.0) }, 0);
        assert_eq!(unsafe { __torajs_num_is_nan_f(1.5) }, 0);
        assert_eq!(unsafe { __torajs_num_is_nan_f(f64::INFINITY) }, 0);
        assert_eq!(unsafe { __torajs_num_is_nan_f(-f64::INFINITY) }, 0);
        assert_eq!(unsafe { __torajs_num_is_nan_i(0) }, 0);
        assert_eq!(unsafe { __torajs_num_is_nan_i(i64::MAX) }, 0);
    }

    #[test]
    fn is_finite_rejects_nan_and_infinities() {
        assert_eq!(unsafe { __torajs_num_is_finite_f(0.0) }, 1);
        assert_eq!(unsafe { __torajs_num_is_finite_f(1.5) }, 1);
        assert_eq!(unsafe { __torajs_num_is_finite_f(f64::NAN) }, 0);
        assert_eq!(unsafe { __torajs_num_is_finite_f(f64::INFINITY) }, 0);
        assert_eq!(unsafe { __torajs_num_is_finite_f(-f64::INFINITY) }, 0);
        assert_eq!(unsafe { __torajs_num_is_finite_i(0) }, 1);
        assert_eq!(unsafe { __torajs_num_is_finite_i(-9_999) }, 1);
    }

    #[test]
    fn is_integer_basic() {
        assert_eq!(unsafe { __torajs_num_is_integer_f(0.0) }, 1);
        assert_eq!(unsafe { __torajs_num_is_integer_f(1.0) }, 1);
        assert_eq!(unsafe { __torajs_num_is_integer_f(-100.0) }, 1);
        assert_eq!(unsafe { __torajs_num_is_integer_f(1.5) }, 0);
        assert_eq!(unsafe { __torajs_num_is_integer_f(f64::NAN) }, 0);
        assert_eq!(unsafe { __torajs_num_is_integer_f(f64::INFINITY) }, 0);
        assert_eq!(unsafe { __torajs_num_is_integer_i(0) }, 1);
    }

    #[test]
    fn is_any_tag_dispatch() {
        // ANY_I64 — always integer & finite, never NaN.
        assert_eq!(unsafe { __torajs_num_is_integer_any(ANY_I64, 42) }, 1);
        assert_eq!(unsafe { __torajs_num_is_finite_any(ANY_I64, 42) }, 1);
        assert_eq!(unsafe { __torajs_num_is_nan_any(ANY_I64, 42) }, 0);
        assert_eq!(
            unsafe { __torajs_num_is_safe_integer_any(ANY_I64, MAX_SAFE_INTEGER_I) },
            1
        );
        assert_eq!(
            unsafe { __torajs_num_is_safe_integer_any(ANY_I64, MAX_SAFE_INTEGER_I + 1) },
            0
        );
        // ANY_F64 — bit-cast the val to f64 and dispatch.
        let nan_bits = f64::NAN.to_bits() as i64;
        assert_eq!(unsafe { __torajs_num_is_nan_any(ANY_F64, nan_bits) }, 1);
        let inf_bits = f64::INFINITY.to_bits() as i64;
        assert_eq!(unsafe { __torajs_num_is_finite_any(ANY_F64, inf_bits) }, 0);
        let one_p_five = 1.5_f64.to_bits() as i64;
        assert_eq!(
            unsafe { __torajs_num_is_integer_any(ANY_F64, one_p_five) },
            0
        );
        let two_p_zero = 2.0_f64.to_bits() as i64;
        assert_eq!(
            unsafe { __torajs_num_is_integer_any(ANY_F64, two_p_zero) },
            1
        );
        // Non-Number tag — always false.
        for tag in [0_i64, 1, 4, 5, 99] {
            assert_eq!(unsafe { __torajs_num_is_nan_any(tag, 0) }, 0);
            assert_eq!(unsafe { __torajs_num_is_finite_any(tag, 0) }, 0);
            assert_eq!(unsafe { __torajs_num_is_integer_any(tag, 0) }, 0);
            assert_eq!(unsafe { __torajs_num_is_safe_integer_any(tag, 0) }, 0);
        }
    }

    #[test]
    fn is_safe_integer_range() {
        assert_eq!(unsafe { __torajs_num_is_safe_integer_f(0.0) }, 1);
        assert_eq!(
            unsafe { __torajs_num_is_safe_integer_f(MAX_SAFE_INTEGER_F) },
            1
        );
        assert_eq!(
            unsafe { __torajs_num_is_safe_integer_f(-MAX_SAFE_INTEGER_F) },
            1
        );
        assert_eq!(
            unsafe { __torajs_num_is_safe_integer_f(MAX_SAFE_INTEGER_F + 1.0) },
            0
        );
        assert_eq!(unsafe { __torajs_num_is_safe_integer_f(1.5) }, 0);
        assert_eq!(unsafe { __torajs_num_is_safe_integer_f(f64::NAN) }, 0);
        assert_eq!(unsafe { __torajs_num_is_safe_integer_i(0) }, 1);
        assert_eq!(
            unsafe { __torajs_num_is_safe_integer_i(MAX_SAFE_INTEGER_I) },
            1
        );
        assert_eq!(
            unsafe { __torajs_num_is_safe_integer_i(MAX_SAFE_INTEGER_I + 1) },
            0
        );
    }
}
