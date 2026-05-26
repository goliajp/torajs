//! NaN-box tagged immediate `AnyValue` — JSC-style 64-bit encoding.
//!
//! Step 7b of the v0.7 Phase-3 plan (see
//! [`docs/v0.7-Phase3-nanbox.md`](../../docs/v0.7-Phase3-nanbox.md)).
//! Introduces a `u64` immediate value type that encodes every JS
//! value without per-primitive allocation. The heap-allocated
//! `AnyBox` struct stays in place during 7b — ssa_lower callsites
//! still emit boxed-allocation IR — and migrates over 7c-7f.
//!
//! ## Layout (JSC double-encode offset)
//!
//! ```text
//! 0x0007_0000_0000_0000     <= encoded f64                  < 0xFFFE_0000_0000_0000
//! 0xFFFE_0000_0000_0000     <= encoded i32  (top 16b == TAG_TYPE_NUMBER)
//! 0x0000_0000_0000_0000      reserved (unused — empty slot)
//! 0x0000_0000_0000_0002      VALUE_NULL      (TAG_BIT_TYPE_OTHER)
//! 0x0000_0000_0000_000A      VALUE_UNDEFINED (TAG_BIT_TYPE_OTHER | TAG_BIT_UNDEFINED)
//! 0x0000_0000_0000_0006      VALUE_FALSE     (TAG_BIT_TYPE_OTHER | TAG_BIT_BOOL)
//! 0x0000_0000_0000_0007      VALUE_TRUE      (TAG_BIT_TYPE_OTHER | TAG_BIT_BOOL | 1)
//! 0x0000_0000_0000_0001 .. 0x0000_FFFF_FFFF_FFFF  encoded *HeapHeader
//!                                          (aarch64 user-VA = 48 bits;
//!                                           top 16b zero; low bit must
//!                                           NOT be set on a real ptr
//!                                           which is 8-aligned)
//! ```
//!
//! Single-conditional fast paths:
//!
//! - `is_int32(v)`  — `top16 == 0xFFFE`           → 1 cmp
//! - `is_double(v)` — `(v & TAG_TYPE_NUMBER) != 0 && !is_int32(v)`
//! - `is_cell(v)`   — `(v & TAG_TYPE_NUMBER) == 0 && (v & TAG_BIT_TYPE_OTHER) == 0 && v != 0`
//! - `is_null`, `is_undefined`, `is_true`, `is_false` — direct equality

use std::ffi::c_void;

use torajs_rc::HeapHeader;

// ============================================================
// AnyValue immediate
// ============================================================

/// 64-bit NaN-box immediate. Encodes any JS value (Int32, f64,
/// Bool, Null, Undefined, heap pointer) without per-primitive
/// allocation. ABI: passed as `i64` / `u64` at the C-extern
/// boundary; the underlying integer is the same value.
pub type AnyValue = u64;

// ============================================================
// JSC-style layout constants
// ============================================================

/// Top-16-bit tag marking the integer as an `i32`. Decoding strips
/// this and sign-extends the low 32 bits.
pub const TAG_TYPE_NUMBER: u64 = 0xFFFE_0000_0000_0000;

/// Additive offset applied to real f64 bits so they land in a
/// contiguous range above sentinels and below the i32 tag. Real
/// f64 NaN has top 16b = `0x7FF8` (quiet) / `0xFFF8` (signaling);
/// after `+ DOUBLE_ENCODE_OFFSET` they shift into `[0x7FFF, 0xFFFE)`
/// — distinct from `TAG_TYPE_NUMBER = 0xFFFE_...`.
pub const DOUBLE_ENCODE_OFFSET: u64 = 0x0007_0000_0000_0000;

/// Bit 1 — set on every Null / Undefined / Bool sentinel. Cleared
/// on every heap pointer (which is 8-aligned ⇒ bottom 3 bits = 0).
pub const TAG_BIT_TYPE_OTHER: u64 = 0x0000_0000_0000_0002;

/// Bit 2 — set on Bool sentinels only. Cleared on Null / Undefined.
pub const TAG_BIT_BOOL: u64 = 0x0000_0000_0000_0004;

/// Bit 3 — set on Undefined only. Cleared on Null and Bools.
pub const TAG_BIT_UNDEFINED: u64 = 0x0000_0000_0000_0008;

/// `false` sentinel = 0x06.
pub const VALUE_FALSE: AnyValue = TAG_BIT_TYPE_OTHER | TAG_BIT_BOOL;

/// `true` sentinel  = 0x07.
pub const VALUE_TRUE: AnyValue = TAG_BIT_TYPE_OTHER | TAG_BIT_BOOL | 1;

/// `null` sentinel  = 0x02.
pub const VALUE_NULL: AnyValue = TAG_BIT_TYPE_OTHER;

/// `undefined` sentinel = 0x0A.
pub const VALUE_UNDEFINED: AnyValue = TAG_BIT_TYPE_OTHER | TAG_BIT_UNDEFINED;

/// Top-16-bit mask isolating the encoding region.
const TOP_16_MASK: u64 = 0xFFFF_0000_0000_0000;

// ============================================================
// Decode primitives — 1-cmp fast paths
// ============================================================

/// `true` iff `v` encodes a 32-bit signed integer.
#[inline]
pub const fn is_int32(v: AnyValue) -> bool {
    (v & TOP_16_MASK) == TAG_TYPE_NUMBER
}

/// `true` iff `v` encodes an IEEE 754 double (any finite, ±0, NaN,
/// ±Inf). The encoded representation is real-f64-bits +
/// `DOUBLE_ENCODE_OFFSET`, so real doubles land in the range
/// `[DOUBLE_ENCODE_OFFSET, TAG_TYPE_NUMBER)`.
#[inline]
pub const fn is_double(v: AnyValue) -> bool {
    (v & TAG_TYPE_NUMBER) != 0 && !is_int32(v)
}

/// `true` iff `v` encodes a heap pointer. Excludes `0` (reserved /
/// unused) and excludes sentinels (Null / Undefined / Bool, which
/// all have `TAG_BIT_TYPE_OTHER` set).
#[inline]
pub const fn is_cell(v: AnyValue) -> bool {
    (v & TAG_TYPE_NUMBER) == 0 && (v & TAG_BIT_TYPE_OTHER) == 0 && v != 0
}

/// `true` iff `v == VALUE_NULL`.
#[inline]
pub const fn is_null(v: AnyValue) -> bool {
    v == VALUE_NULL
}

/// `true` iff `v == VALUE_UNDEFINED`.
#[inline]
pub const fn is_undefined(v: AnyValue) -> bool {
    v == VALUE_UNDEFINED
}

/// `true` iff `v` is `VALUE_FALSE` or `VALUE_TRUE`.
#[inline]
pub const fn is_bool(v: AnyValue) -> bool {
    (v & !1u64) == VALUE_FALSE
}

#[inline]
pub const fn is_true(v: AnyValue) -> bool {
    v == VALUE_TRUE
}

#[inline]
pub const fn is_false(v: AnyValue) -> bool {
    v == VALUE_FALSE
}

/// `true` for any value that's NOT a heap pointer (primitives only).
#[inline]
pub const fn is_primitive(v: AnyValue) -> bool {
    !is_cell(v)
}

// ============================================================
// Extract primitives — caller asserts the matching tag predicate
// ============================================================

/// Caller asserts [`is_int32`]. Low 32 bits sign-extended to i32.
#[inline]
pub const fn as_int32(v: AnyValue) -> i32 {
    v as i32
}

/// Caller asserts [`is_double`]. Subtract the encode offset, then
/// reinterpret as `f64` bits.
#[inline]
pub fn as_double(v: AnyValue) -> f64 {
    f64::from_bits(v.wrapping_sub(DOUBLE_ENCODE_OFFSET))
}

/// Caller asserts [`is_cell`]. 48-bit aarch64 user-VA fits in a
/// raw pointer cast verbatim.
#[inline]
pub const fn as_pointer(v: AnyValue) -> *mut HeapHeader {
    v as *mut HeapHeader
}

/// Caller asserts [`is_cell`]. Returns the pointer as `*mut c_void`
/// for FFI sites that don't need the typed `HeapHeader`.
#[inline]
pub const fn as_void_ptr(v: AnyValue) -> *mut c_void {
    v as *mut c_void
}

/// Caller asserts [`is_bool`]. Low bit = truthy / falsy.
#[inline]
pub const fn as_bool(v: AnyValue) -> bool {
    (v & 1) != 0
}

// ============================================================
// Encode primitives
// ============================================================

/// Encode an `i32` as a tagged AnyValue. Top 16 bits become
/// `TAG_TYPE_NUMBER`; low 32 bits carry the payload (zero-
/// extended; sign-bit interpretation kicks in on extraction).
#[inline]
pub const fn box_int32(x: i32) -> AnyValue {
    TAG_TYPE_NUMBER | (x as u32 as u64)
}

/// Encode an `f64` via the offset trick.
#[inline]
pub fn box_double(x: f64) -> AnyValue {
    f64::to_bits(x).wrapping_add(DOUBLE_ENCODE_OFFSET)
}

/// Encode a heap pointer. The aarch64 user-VA is 48 bits, so the
/// top 16 bits are already zero; no masking needed. The pointer
/// must be 8-aligned (every `HeapHeader` is) — that keeps the
/// `TAG_BIT_TYPE_OTHER` bit clear so [`is_cell`] picks it up
/// correctly.
#[inline]
pub fn box_pointer(p: *mut HeapHeader) -> AnyValue {
    p as u64
}

/// Encode a void heap pointer (for the C-side `*mut c_void` FFI
/// path that doesn't carry a `HeapHeader` type).
#[inline]
pub fn box_void_ptr(p: *mut c_void) -> AnyValue {
    p as u64
}

/// Encode a `bool` as the matching sentinel.
#[inline]
pub const fn box_bool(b: bool) -> AnyValue {
    if b { VALUE_TRUE } else { VALUE_FALSE }
}

// ============================================================
// Tests
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ----- sentinel values -----

    #[test]
    fn sentinel_constants_match_jsc() {
        // JSC reference values from JSValue.h.
        assert_eq!(VALUE_NULL, 0x02);
        assert_eq!(VALUE_FALSE, 0x06);
        assert_eq!(VALUE_TRUE, 0x07);
        assert_eq!(VALUE_UNDEFINED, 0x0A);
    }

    #[test]
    fn null_is_only_null() {
        assert!(is_null(VALUE_NULL));
        assert!(!is_undefined(VALUE_NULL));
        assert!(!is_bool(VALUE_NULL));
        assert!(!is_cell(VALUE_NULL));
        assert!(!is_int32(VALUE_NULL));
        assert!(!is_double(VALUE_NULL));
        assert!(is_primitive(VALUE_NULL));
    }

    #[test]
    fn undefined_is_only_undefined() {
        assert!(is_undefined(VALUE_UNDEFINED));
        assert!(!is_null(VALUE_UNDEFINED));
        assert!(!is_bool(VALUE_UNDEFINED));
        assert!(!is_cell(VALUE_UNDEFINED));
        assert!(is_primitive(VALUE_UNDEFINED));
    }

    #[test]
    fn bool_predicate_covers_both_sentinels() {
        assert!(is_bool(VALUE_FALSE));
        assert!(is_bool(VALUE_TRUE));
        assert!(is_false(VALUE_FALSE));
        assert!(!is_false(VALUE_TRUE));
        assert!(is_true(VALUE_TRUE));
        assert!(!is_true(VALUE_FALSE));
        assert_eq!(as_bool(VALUE_FALSE), false);
        assert_eq!(as_bool(VALUE_TRUE), true);
        assert!(!is_null(VALUE_TRUE));
        assert!(!is_undefined(VALUE_TRUE));
        assert!(!is_cell(VALUE_TRUE));
    }

    // ----- int32 round trip -----

    #[test]
    fn box_unbox_int32_zero() {
        let v = box_int32(0);
        assert!(is_int32(v));
        assert!(!is_double(v));
        assert!(!is_cell(v));
        assert_eq!(as_int32(v), 0);
    }

    #[test]
    fn box_unbox_int32_positive() {
        for x in [1i32, 7, 42, 0x0BAD_F00D, i32::MAX] {
            let v = box_int32(x);
            assert!(is_int32(v), "{x:#x} encoded as {v:#x}");
            assert_eq!(as_int32(v), x);
            assert!(!is_double(v));
            assert!(!is_cell(v));
        }
    }

    #[test]
    fn box_unbox_int32_negative() {
        for x in [-1i32, -42, i32::MIN, -0x0DEAD_BEEFi64 as i32] {
            let v = box_int32(x);
            assert!(is_int32(v), "{x} encoded as {v:#x}");
            assert_eq!(as_int32(v), x);
        }
    }

    // ----- f64 round trip -----

    #[test]
    fn box_unbox_f64_finite() {
        for x in [
            0.0f64,
            -0.0,
            1.0,
            -1.0,
            std::f64::consts::PI,
            std::f64::consts::E,
            1e308,
            -1e308,
            1e-308,
        ] {
            let v = box_double(x);
            assert!(is_double(v), "{x} encoded as {v:#x}");
            assert!(!is_int32(v));
            assert!(!is_cell(v));
            assert_eq!(as_double(v).to_bits(), x.to_bits());
        }
    }

    #[test]
    fn box_unbox_f64_special() {
        let inf = box_double(f64::INFINITY);
        assert!(is_double(inf));
        assert!(as_double(inf).is_infinite() && as_double(inf).is_sign_positive());

        let ninf = box_double(f64::NEG_INFINITY);
        assert!(is_double(ninf));
        assert!(as_double(ninf).is_infinite() && as_double(ninf).is_sign_negative());

        let nan = box_double(f64::NAN);
        assert!(is_double(nan));
        assert!(as_double(nan).is_nan());
    }

    // ----- pointer round trip -----

    #[test]
    fn box_unbox_pointer() {
        // Use a stack-allocated HeapHeader for shape-only round-
        // trip. Address comes from address-of-mut on a real
        // HeapHeader so it's 8-aligned and within user-VA.
        use torajs_rc::Tag;
        let mut h = HeapHeader::new(Tag::Str);
        let p = &mut h as *mut HeapHeader;
        let v = box_pointer(p);
        assert!(is_cell(v));
        assert!(!is_int32(v));
        assert!(!is_double(v));
        assert!(!is_null(v));
        assert!(!is_undefined(v));
        assert!(!is_bool(v));
        assert_eq!(as_pointer(v), p);
    }

    #[test]
    fn cell_zero_is_not_cell() {
        // v == 0 is reserved (no encoding uses it).
        assert!(!is_cell(0));
    }

    // ----- disjoint encoding spaces -----

    #[test]
    fn int32_max_does_not_collide_with_double_range() {
        let v_i32_max = box_int32(i32::MAX);
        // Top 16b == 0xFFFE — distinct from any encoded double
        // (encoded doubles top16 ∈ [0x0007, 0xFFFE)).
        assert!(is_int32(v_i32_max));
        assert!(!is_double(v_i32_max));
    }

    #[test]
    fn double_nan_does_not_alias_int32_tag() {
        // Real f64 NaN bits: 0x7FF8_0000_0000_0000 (quiet).
        // After + DOUBLE_ENCODE_OFFSET = 0x7FFF_0000_0000_0000
        // — top16 = 0x7FFF, not 0xFFFE.
        let v_nan = box_double(f64::NAN);
        assert!(is_double(v_nan));
        assert!(!is_int32(v_nan));
        // signaling NaN (negative-sign quiet NaN bits) also OK:
        let s_nan_bits = 0xFFF8_0000_0000_0001u64;
        let s_nan = f64::from_bits(s_nan_bits);
        let v_s = box_double(s_nan);
        assert!(is_double(v_s));
        // After + DOUBLE_ENCODE_OFFSET, top16 ≥ 0x7FFF... but
        // wraps within u64 — verify it's still classified as a
        // double via the predicate, not an i32.
        assert!(!is_int32(v_s));
    }

    #[test]
    fn predicates_are_mutually_exclusive() {
        // Every encoding fits exactly one of: int32, double,
        // cell, null, undefined, bool.
        let samples: &[AnyValue] = &[
            VALUE_NULL,
            VALUE_UNDEFINED,
            VALUE_TRUE,
            VALUE_FALSE,
            box_int32(0),
            box_int32(42),
            box_int32(-1),
            box_int32(i32::MAX),
            box_int32(i32::MIN),
            box_double(0.0),
            box_double(3.14),
            box_double(f64::NAN),
            box_double(f64::INFINITY),
        ];
        for &v in samples {
            let flags = [
                is_int32(v),
                is_double(v),
                is_cell(v),
                is_null(v),
                is_undefined(v),
                is_bool(v),
            ];
            let count = flags.iter().filter(|&&b| b).count();
            assert_eq!(
                count, 1,
                "value {v:#x} matched {count} predicates: {flags:?}"
            );
        }
    }

    #[test]
    fn box_bool_helper_returns_sentinels() {
        assert_eq!(box_bool(true), VALUE_TRUE);
        assert_eq!(box_bool(false), VALUE_FALSE);
    }
}
