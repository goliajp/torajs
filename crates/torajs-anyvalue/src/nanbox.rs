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
pub const TOP_16_MASK: u64 = 0xFFFF_0000_0000_0000;

// ============================================================
// ShortStr (Step 8 — Small-String Optimization)
//
// Layout (Step 8a design doc, `docs/v0.7-Phase3-Step8-sso.md`):
//
//   bits 63..48: top16 = 0x0001       (marker, claims unused
//                                      top16 = 0x0001 region —
//                                      single safe marker; see
//                                      doc "Bit-space audit")
//   bits 47..40: len   = 0..5         (8-bit len, future-proof)
//   bits 39..0 : bytes = 5 × 8 bit    (little-endian payload)
//
// Capacity 5 bytes. Covers "true"/"false"/"null"/"NaN", 1-5
// digit decimal Number.toString, single ASCII char, JSON
// tokens / delimiters. Does NOT cover "undefined" (9 byte) /
// "[object]" (8 byte) / multibyte UTF-8 ≥ 6 byte.
// ============================================================

/// Marker for ShortStr-encoded `AnyValue`: `top16 == 0x0001`.
/// Forced single-marker by NaN-box bit budget — see
/// `docs/v0.7-Phase3-Step8-sso.md` "Bit-space audit".
pub const SHORT_STR_TAG: u64 = 0x0001_0000_0000_0000;

/// Maximum inline byte payload for ShortStr. Driven by the 48-bit
/// low payload split: 8-bit len + 40-bit (5 × 8) bytes.
pub const SHORT_STR_CAP: usize = 5;

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
/// unused), excludes sentinels (Null / Undefined / Bool — all
/// carry `TAG_BIT_TYPE_OTHER`), AND excludes ShortStr (top16 =
/// 0x0001 — Step 8 SSO).
///
/// **Step 8 tighten:** the strict `(v & TOP_16_MASK) == 0` check
/// replaces the pre-Step-8 weak `(v & TAG_TYPE_NUMBER) == 0`. The
/// weak check let `top16 == 0x0001` through (since
/// `0x0001 & 0xFFFE == 0`); combined with a ShortStr payload that
/// happens to clear bit 1 (e.g. `"a"` = `0x61`), the weak check
/// would misclassify ShortStr as a cell pointer. The tighten is a
/// no-op refactor when no ShortStr values exist (top16 = 0x0001
/// never occurs pre-8b) and the strict predicate cleanly
/// distinguishes ShortStr from cell once 8c starts producing
/// ShortStr values.
#[inline]
pub const fn is_cell(v: AnyValue) -> bool {
    (v & TOP_16_MASK) == 0 && (v & TAG_BIT_TYPE_OTHER) == 0 && v != 0
}

/// `true` iff `v` encodes a ShortStr (Step 8 SSO). Single-cmp
/// predicate via `top16 == 0x0001` check. Mutually exclusive
/// with `is_int32` / `is_double` / `is_cell` / sentinels.
#[inline]
pub const fn is_short_str(v: AnyValue) -> bool {
    (v & TOP_16_MASK) == SHORT_STR_TAG
}

/// Length (in bytes, 0..[`SHORT_STR_CAP`]) of a ShortStr-encoded
/// value. Reads bits 47..40 of `v`.
///
/// Caller asserts [`is_short_str`]. Returns 0..255 byte; values >
/// `SHORT_STR_CAP` indicate a malformed encoding (defensive — 8c
/// producer paths enforce the bound at construction).
#[inline]
pub const fn short_str_len(v: AnyValue) -> u8 {
    ((v >> 40) & 0xFF) as u8
}

/// Decode the 5-byte payload of a ShortStr. Bytes are little-
/// endian: byte 0 lives in low 8 bits of `v`. Unused trailing
/// bytes (when `len < SHORT_STR_CAP`) read as 0.
///
/// Caller asserts [`is_short_str`].
#[inline]
pub const fn short_str_bytes(v: AnyValue) -> [u8; SHORT_STR_CAP] {
    let payload = v & 0x0000_00FF_FFFF_FFFF;
    [
        (payload) as u8,
        (payload >> 8) as u8,
        (payload >> 16) as u8,
        (payload >> 24) as u8,
        (payload >> 32) as u8,
    ]
}

/// Try to encode a byte slice as a ShortStr `AnyValue`. Returns
/// `None` when `bytes.len() > SHORT_STR_CAP` — the caller falls
/// back to Heap+Str alloc in that case.
///
/// Bytes are stored little-endian: `bytes[0]` → low 8 bits, etc.
/// 0-length is a valid ShortStr (`""`).
#[inline]
pub fn try_box_short_str(bytes: &[u8]) -> Option<AnyValue> {
    if bytes.len() > SHORT_STR_CAP {
        return None;
    }
    let mut payload: u64 = 0;
    for (i, &b) in bytes.iter().enumerate() {
        payload |= (b as u64) << (i * 8);
    }
    Some(SHORT_STR_TAG | ((bytes.len() as u64) << 40) | payload)
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

/// `true` for any value that's NOT a heap pointer. After Step 8
/// SSO, this includes ShortStr-encoded values (which carry their
/// bytes inline — no heap allocation).
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
///
/// Step 8c — NaN-payload collision guard. Real f64 bits in
/// `[0, 0xFFF8_..)` encode cleanly into `[0x0007_.., 0xFFFE_..)`,
/// where [`is_double`] is true. But certain NaN payloads (negative
/// NaN with `raw_top16 ∈ [0xFFF7_.., 0xFFFF_..]`) wrap their
/// encoded form into non-double bit ranges:
///
/// | Raw top16 | Encoded top16 | Mis-classification              |
/// |-----------|---------------|----------------------------------|
/// | 0xFFF7    | 0xFFFE        | int32 tag — `is_int32 == true`   |
/// | 0xFFF9    | 0x0000        | cell range (or reserved 0)       |
/// | 0xFFFA    | 0x0001        | ShortStr tag                     |
/// | 0xFFFB    | 0x0002        | VALUE_NULL / sentinel range      |
/// | 0xFFFC-FE | 0x0003-0005   | sentinel space                   |
/// | 0xFFFF    | 0x0006        | VALUE_FALSE                      |
///
/// User code can construct such NaN payloads via Float64Array
/// bit-level views (V8 / JSC / SpiderMonkey all canonicalize at
/// the equivalent boundary). The fix: any encoded value for which
/// [`is_double`] is false must be the result of NaN-collision —
/// re-emit it as the canonical `f64::NAN` bit pattern, which
/// encodes safely (top16 = 0x7FFF, [`is_double`] true). Single
/// `!is_double` predicate covers all collision regions in one
/// branch the optimizer collapses to a single mask + cmov.
#[inline]
pub fn box_double(x: f64) -> AnyValue {
    let encoded = f64::to_bits(x).wrapping_add(DOUBLE_ENCODE_OFFSET);
    if !is_double(encoded) {
        return f64::to_bits(f64::NAN).wrapping_add(DOUBLE_ENCODE_OFFSET);
    }
    encoded
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
    fn box_double_canonicalizes_short_str_collision_nan() {
        // Step 8c — ShortStr collision case (handoff Risk #2). A
        // NaN with payload bits `0xFFFA_0000_0000_0000` would, after
        // + DOUBLE_ENCODE_OFFSET and u64 wrap, encode to
        // `0x0001_0000_0000_0000` — identical to `SHORT_STR_TAG`.
        // Without canonicalization, `is_short_str` would fire on
        // what should be an f64 NaN.
        let collision_bits = 0xFFFA_0000_0000_0000u64;
        let collision_f = f64::from_bits(collision_bits);
        assert!(collision_f.is_nan(), "construction must be a NaN");
        let v = box_double(collision_f);
        assert!(
            is_double(v),
            "collision NaN must encode as double, got {v:#x}"
        );
        assert!(
            !is_short_str(v),
            "collision NaN must not encode as ShortStr"
        );
        assert!(as_double(v).is_nan(), "decoded value must still be NaN");
    }

    #[test]
    fn box_double_canonicalizes_entire_negative_nan_collision_range() {
        // Negative-NaN payloads cover **multiple** collision
        // regions, not just ShortStr (see [`box_double`] doc
        // table). Verify each region canonicalizes and the
        // decoded value is still a NaN — silent mis-classification
        // as int32 / cell / sentinel / ShortStr is the bug class
        // we're killing.
        for (raw, expected_collision) in [
            (0xFFF7_0000_0000_0000u64, "int32 tag"),
            (0xFFF7_FFFF_FFFF_FFFFu64, "int32 tag (high)"),
            (0xFFF9_0000_0000_0001u64, "cell range (low)"),
            (0xFFF9_DEAD_BEEF_F00Du64, "cell range (mid)"),
            (0xFFFA_0500_0000_0000u64, "ShortStr len=5 shape"),
            (0xFFFA_FFFF_FFFF_FFFFu64, "ShortStr (high)"),
            (0xFFFB_0000_0000_0000u64, "VALUE_NULL sentinel"),
            (0xFFFC_0000_0000_0000u64, "sentinel range"),
            (0xFFFD_0000_0000_0000u64, "sentinel range"),
            (0xFFFE_0000_0000_0000u64, "sentinel range"),
            (0xFFFF_0000_0000_0006u64, "VALUE_FALSE shape"),
        ] {
            let f = f64::from_bits(raw);
            assert!(
                f.is_nan(),
                "raw={raw:#x} ({expected_collision}) must be NaN"
            );
            let v = box_double(f);
            assert!(
                is_double(v),
                "raw={raw:#x} ({expected_collision}) encoded={v:#x} must classify as double"
            );
            assert!(
                !is_short_str(v) && !is_int32(v) && !is_cell(v) && !is_null(v) && !is_bool(v),
                "raw={raw:#x} ({expected_collision}) encoded={v:#x} mis-classified"
            );
            assert!(
                as_double(v).is_nan(),
                "raw={raw:#x} ({expected_collision}) decoded must remain NaN"
            );
        }
    }

    #[test]
    fn box_double_finite_and_canonical_nan_round_trip_unchanged() {
        // Real finite f64 values + canonical quiet NaN encode
        // cleanly without canonicalization; the round-trip must
        // preserve bits exactly. Regression guard against the
        // canonicalization branch over-firing.
        for raw in [
            0x0000_0000_0000_0000u64, // +0.0
            0x8000_0000_0000_0000u64, // -0.0
            0x3FF0_0000_0000_0000u64, // 1.0
            0xBFF0_0000_0000_0000u64, // -1.0
            0x4000_0000_0000_0000u64, // 2.0
            0x7FF0_0000_0000_0000u64, // +Infinity
            0xFFF0_0000_0000_0000u64, // -Infinity
            0x7FF8_0000_0000_0000u64, // canonical quiet NaN
        ] {
            let f = f64::from_bits(raw);
            let v = box_double(f);
            assert!(is_double(v), "raw={raw:#x} encoded={v:#x}");
            assert!(!is_short_str(v) && !is_int32(v) && !is_cell(v));
            // Round-trip preserves bits for all non-collision
            // cases (including canonical NaN).
            assert_eq!(
                as_double(v).to_bits(),
                raw,
                "round-trip must preserve bits for raw={raw:#x}"
            );
        }
    }

    #[test]
    fn predicates_are_mutually_exclusive() {
        // Every encoding fits exactly one of: int32, double,
        // cell, null, undefined, bool, short_str (Step 8).
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
            // Step 8 ShortStr coverage
            try_box_short_str(b"").unwrap(),
            try_box_short_str(b"a").unwrap(),
            try_box_short_str(b"abcde").unwrap(),
            try_box_short_str(b"true").unwrap(),
        ];
        for &v in samples {
            let flags = [
                is_int32(v),
                is_double(v),
                is_cell(v),
                is_null(v),
                is_undefined(v),
                is_bool(v),
                is_short_str(v),
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

    // ----- ShortStr (Step 8 SSO) -----

    #[test]
    fn short_str_empty_round_trip() {
        let v = try_box_short_str(b"").unwrap();
        assert!(is_short_str(v));
        assert!(!is_cell(v));
        assert!(!is_int32(v));
        assert!(!is_double(v));
        assert!(!is_null(v));
        assert!(!is_undefined(v));
        assert!(!is_bool(v));
        assert!(is_primitive(v));
        assert_eq!(short_str_len(v), 0);
        assert_eq!(short_str_bytes(v), [0, 0, 0, 0, 0]);
    }

    #[test]
    fn short_str_one_byte_ascii_round_trip() {
        let v = try_box_short_str(b"a").unwrap();
        assert!(is_short_str(v));
        assert!(!is_cell(v), "'a' (0x61, bit-1 clear) must NOT be cell");
        assert_eq!(short_str_len(v), 1);
        assert_eq!(short_str_bytes(v)[0], b'a');
        assert_eq!(short_str_bytes(v)[1..], [0, 0, 0, 0]);
    }

    #[test]
    fn short_str_max_capacity_round_trip() {
        // "abcde" — exact 5-byte fit
        let v = try_box_short_str(b"abcde").unwrap();
        assert!(is_short_str(v));
        assert_eq!(short_str_len(v), 5);
        assert_eq!(short_str_bytes(v), *b"abcde");
    }

    #[test]
    fn short_str_too_long_returns_none() {
        // "abcdef" — 6 byte, exceeds SHORT_STR_CAP
        assert!(try_box_short_str(b"abcdef").is_none());
        assert!(try_box_short_str(b"undefined").is_none());
        assert!(try_box_short_str(b"[object]").is_none());
    }

    #[test]
    fn short_str_bit1_clear_payload_not_misclassified_as_cell() {
        // Regression for the is_cell tightening rationale: ShortStr
        // values whose byte 0 has bit 1 = 0 (e.g. 'a' = 0x61,
        // ' ' = 0x20, '0' = 0x30) must NOT pass the cell predicate.
        // Pre-tighten weak check would have let them through.
        for &b in &[b'a', b' ', b'0', b'8', b'@', b'`'] {
            let v = try_box_short_str(&[b]).unwrap();
            assert!(!is_cell(v), "byte {b:#x} ShortStr leaked as cell");
            assert!(is_short_str(v));
        }
    }

    #[test]
    fn short_str_bit1_set_payload_also_classified_correctly() {
        // Bytes with bit 1 = 1 (e.g. 'b' = 0x62, '"' = 0x22) — also
        // must be ShortStr, not cell. The bit-1 check only matters
        // for top16=0 (sentinels vs cell); top16=0x0001 makes the
        // TOP_16_MASK check fail before bit-1 is read.
        for &b in &[b'b', b'"', b'2', b'F', b'B'] {
            let v = try_box_short_str(&[b]).unwrap();
            assert!(!is_cell(v));
            assert!(is_short_str(v));
        }
    }

    #[test]
    fn short_str_distinct_from_aarch64_user_va() {
        // Real cell pointers on aarch64 are 48-bit user-VA, 8-
        // aligned. top16 == 0x0000. ShortStr top16 == 0x0001.
        // Constructing a maximum-aligned 48-bit pointer-like value
        // and verifying ShortStr's tag-bit distinguishes them.
        let fake_cell: AnyValue = 0x0000_FFFF_FFFF_FFF8; // 48-bit max, 8-aligned
        assert!(is_cell(fake_cell));
        assert!(!is_short_str(fake_cell));

        let v = try_box_short_str(b"abcde").unwrap();
        assert!(is_short_str(v));
        assert!(!is_cell(v));
        assert_ne!(v & TOP_16_MASK, fake_cell & TOP_16_MASK);
    }

    #[test]
    fn short_str_byte_order_is_little_endian() {
        // Encoding "ABC" should put 'A' (0x41) in the low byte of
        // the payload, 'B' (0x42) at +8, 'C' (0x43) at +16.
        let v = try_box_short_str(b"ABC").unwrap();
        let payload = v & 0x0000_00FF_FFFF_FFFF;
        assert_eq!(payload & 0xFF, 0x41);
        assert_eq!((payload >> 8) & 0xFF, 0x42);
        assert_eq!((payload >> 16) & 0xFF, 0x43);
    }

    #[test]
    fn short_str_sentinel_values_not_misclassified() {
        // null / undefined / true / false sentinels all live at
        // top16 == 0x0000 with specific low-bit patterns. ShortStr
        // top16 == 0x0001 — no overlap. Verify each.
        for sentinel in [VALUE_NULL, VALUE_UNDEFINED, VALUE_TRUE, VALUE_FALSE] {
            assert!(!is_short_str(sentinel));
        }
        // ShortStr values do NOT pass any sentinel predicate.
        let v = try_box_short_str(b"x").unwrap();
        assert!(!is_null(v));
        assert!(!is_undefined(v));
        assert!(!is_bool(v));
    }
}
