//! Map key equality — SameValueZero (ES spec §7.2.10).
//!
//! Port of `runtime_map.c::map_keys_equal` (P4.3-b, 2026-05-23).
//! Used by [`crate::probe::map_lookup_slot`] (P4.3-c) + the get/has/
//! set/delete extern fns (P4.3-d..-f).
//!
//! Equality rules:
//! - `null === null`, `undefined === undefined`.
//! - `bool` / `i64`: bitwise eq of payload.
//! - `f64`: `NaN === NaN` (both NaN → equal), `+0 === -0` (IEEE eq).
//! - `i64` × `f64` cross-tag: JS Number is one type — the tag split is
//!   a torajs internal representation detail, so an integral f64 key
//!   equals the same-valued i64 key (mirrors the hash canonicalization
//!   in [`crate::hash::map_hash_key`]).
//! - `heap`:
//!   - Pointer-identity short-circuit (interned Str literals + same-
//!     object cases). NULL handled.
//!   - Different type_tag → unequal.
//!   - Both `Str` → byte-by-byte compare via cross-tier
//!     `__torajs_str_eq`.
//!   - Both `BigInt` → by-value compare (sign + limbs), per
//!     SameValueZero — `new Set([1n]).has(1n)` must hit.
//!   - Other heap types → pointer identity only (already short-
//!     circuited above; returns false here).

use core::ffi::c_void;

use crate::hash::f64_as_exact_i64;
use crate::layout::{
    ANY_BOOL, ANY_F64, ANY_HEAP, ANY_I64, ANY_NULL, ANY_UNDEF, BIGINT_LEN_OFF, BIGINT_SIGN_OFF,
    BIGINT_WORDS_OFF, HeapHeader, TAG_BIGINT, TAG_STR,
};

unsafe extern "C" {
    /// Cross-tier — torajs-str's content equality. Returns 1 iff
    /// `a` + `b` are both live Str blocks with identical bytes.
    fn __torajs_str_eq(a: *const u8, b: *const u8) -> i64;
}

/// SameValueZero comparison between two Any-tagged keys.
///
/// # Safety
/// For `ANY_HEAP` tag, payloads are either NULL or valid live heap
/// pointers with universal headers (type_tag read at offset 4).
pub(crate) unsafe fn map_keys_equal(ta: u8, pa: u64, tb: u8, pb: u64) -> bool {
    if ta != tb {
        // Numeric cross-tag: an integral f64 equals the same-valued i64.
        return match (ta, tb) {
            (ANY_I64, ANY_F64) => f64_as_exact_i64(f64::from_bits(pb)) == Some(pa as i64),
            (ANY_F64, ANY_I64) => f64_as_exact_i64(f64::from_bits(pa)) == Some(pb as i64),
            _ => false,
        };
    }
    match ta {
        ANY_NULL | ANY_UNDEF => true,
        ANY_BOOL | ANY_I64 => pa == pb,
        ANY_F64 => {
            let da = f64::from_bits(pa);
            let db = f64::from_bits(pb);
            if da.is_nan() {
                db.is_nan()
            } else if db.is_nan() {
                false
            } else {
                // IEEE eq: +0 == -0 holds here.
                da == db
            }
        }
        ANY_HEAP => {
            let pa_p = pa as *mut c_void;
            let pb_p = pb as *mut c_void;
            if pa_p == pb_p {
                return true;
            }
            if pa_p.is_null() || pb_p.is_null() {
                return false;
            }
            let ha = pa_p as *const HeapHeader;
            let hb = pb_p as *const HeapHeader;
            let ta = unsafe { (*ha).type_tag };
            let tb = unsafe { (*hb).type_tag };
            if ta != tb {
                return false;
            }
            if ta == TAG_STR {
                unsafe { __torajs_str_eq(pa_p as *const u8, pb_p as *const u8) != 0 }
            } else if ta == TAG_BIGINT {
                unsafe { bigint_values_equal(pa_p as *const u8, pb_p as *const u8) }
            } else {
                // Non-Str heap: identity already checked above.
                false
            }
        }
        _ => pa == pb,
    }
}

/// By-value BigInt comparison: sign + limb count + limb bytes. The
/// canonical invariant (no leading zero limbs, `len == 0` ⇒ `sign == 0`)
/// makes representation equality coincide with value equality.
///
/// # Safety
/// `a` + `b` are live BigInt heap blocks (type_tag already checked).
unsafe fn bigint_values_equal(a: *const u8, b: *const u8) -> bool {
    let sign_a = unsafe { *(a.add(BIGINT_SIGN_OFF) as *const u32) };
    let sign_b = unsafe { *(b.add(BIGINT_SIGN_OFF) as *const u32) };
    if sign_a != sign_b {
        return false;
    }
    let len_a = unsafe { *(a.add(BIGINT_LEN_OFF) as *const u32) };
    let len_b = unsafe { *(b.add(BIGINT_LEN_OFF) as *const u32) };
    if len_a != len_b {
        return false;
    }
    let wa = unsafe { a.add(BIGINT_WORDS_OFF) as *const u64 };
    let wb = unsafe { b.add(BIGINT_WORDS_OFF) as *const u64 };
    for i in 0..len_a as usize {
        if unsafe { *wa.add(i) != *wb.add(i) } {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{BIGINT_LEN_OFF, BIGINT_SIGN_OFF, BIGINT_WORDS_OFF, TAG_BIGINT};

    #[test]
    fn cross_tag_integral_f64_equals_i64() {
        for v in [0i64, 7, -3, 9007199254740991, i64::MIN] {
            let fbits = (v as f64).to_bits();
            assert!(unsafe { map_keys_equal(ANY_I64, v as u64, ANY_F64, fbits) });
            assert!(unsafe { map_keys_equal(ANY_F64, fbits, ANY_I64, v as u64) });
        }
        // -0.0 equals i64 0 under SameValueZero.
        assert!(unsafe { map_keys_equal(ANY_I64, 0, ANY_F64, (-0.0f64).to_bits()) });
    }

    #[test]
    fn cross_tag_non_integral_or_out_of_range_unequal() {
        assert!(!unsafe { map_keys_equal(ANY_I64, 7, ANY_F64, 7.5f64.to_bits()) });
        assert!(!unsafe { map_keys_equal(ANY_I64, 7, ANY_F64, f64::NAN.to_bits()) });
        // f64 2^63 saturates to i64::MAX on cast — must NOT equal it.
        let two_pow_63 = 9_223_372_036_854_775_808.0f64.to_bits();
        assert!(!unsafe { map_keys_equal(ANY_I64, i64::MAX as u64, ANY_F64, two_pow_63) });
        // Non-numeric cross-tag stays unequal.
        assert!(!unsafe { map_keys_equal(ANY_BOOL, 0, ANY_I64, 0) });
    }

    // Synthesize a BigInt heap block: [hdr:8][sign:4][len:4][words:8×n].
    fn make_bigint(sign: u32, words: &[u64]) -> Vec<u8> {
        let mut v = vec![0u8; BIGINT_WORDS_OFF + words.len() * 8];
        unsafe {
            *(v.as_mut_ptr().add(4) as *mut u16) = TAG_BIGINT;
            *(v.as_mut_ptr().add(BIGINT_SIGN_OFF) as *mut u32) = sign;
            *(v.as_mut_ptr().add(BIGINT_LEN_OFF) as *mut u32) = words.len() as u32;
            core::ptr::copy_nonoverlapping(
                words.as_ptr() as *const u8,
                v.as_mut_ptr().add(BIGINT_WORDS_OFF),
                words.len() * 8,
            );
        }
        v
    }

    #[test]
    fn bigint_keys_compare_by_value() {
        let a = make_bigint(0, &[1]);
        let b = make_bigint(0, &[1]);
        let c = make_bigint(1, &[1]);
        let d = make_bigint(0, &[2]);
        let e = make_bigint(0, &[1, 1]);
        let eq = |x: &[u8], y: &[u8]| unsafe {
            map_keys_equal(ANY_HEAP, x.as_ptr() as u64, ANY_HEAP, y.as_ptr() as u64)
        };
        assert!(eq(&a, &b), "same value, different allocation");
        assert!(!eq(&a, &c), "sign differs");
        assert!(!eq(&a, &d), "magnitude differs");
        assert!(!eq(&a, &e), "limb count differs");
        // Hash must agree with eq: equal values hash the same.
        let ha = unsafe { crate::hash::map_hash_key(ANY_HEAP, a.as_ptr() as u64) };
        let hb = unsafe { crate::hash::map_hash_key(ANY_HEAP, b.as_ptr() as u64) };
        assert_eq!(ha, hb);
    }
}
