//! NaN-box FFI shims — `__torajs_anyv_*` ABI for the immediate
//! `AnyValue` type (Step 7 cutover, see
//! [`docs/v0.7-Phase3-nanbox.md`](../../docs/v0.7-Phase3-nanbox.md)).
//!
//! The canonical FFI surface ssa_lower binds against. Pre-Step-7
//! `__torajs_any_*` boxed shims were deleted in 7f-D-1; the inner
//! `any_arith` / `any_add` impls were rewritten to return
//! `AnyValue` directly in 7f-D-2 (no transitional AnyBox alloc).
//!
//! Implementation:
//!
//! - **Primitive paths** (Int32 / f64 / Bool / Null / Undefined)
//!   run logic directly on the immediate — zero allocation.
//! - **Cell path** decodes the pointer + dispatches to the
//!   inner helpers (`any_to_number`, `any_to_str`, `any_compare`,
//!   `any_arith`, `any_add`) by synthesizing a `(Heap, ptr)`
//!   pair; the inner helpers retain the tag-table shape for the
//!   compare / coerce paths that don't benefit from immediate
//!   decoding (Str byte compare, ToString allocations).

use std::ffi::c_void;

use torajs_rc::{__torajs_rc_inc, AnySlotTag, Tag};

use crate::arith::{any_add, any_arith};
use crate::coerce::{any_to_number, any_to_str};
use crate::compare::any_compare;
use crate::nanbox::{
    AnyValue, as_bool, as_double, as_int32, as_pointer, as_void_ptr, is_bool, is_cell, is_double,
    is_int32, is_null, is_short_str, is_undefined, short_str_len,
};
use crate::nanbox_ffi_materialize::{
    drop_materialized_str, materialize_if_short, materialize_short_str,
};

// ============================================================
// External C symbols re-declared here so the shims don't depend
// on the lib.rs-private extern block.
// ============================================================

unsafe extern "C" {
    fn __torajs_str_eq(a: *const u8, b: *const u8) -> i64;
    fn __torajs_value_drop_heap(child: *mut c_void);
    /// Heap+Str → number parse. Used by ShortStr ToNumber after
    /// materialize.
    fn __torajs_str_to_number(p: *const c_void) -> f64;
}

// ============================================================
// RC management
// ============================================================

/// Refcount-increment the heap payload of an [`AnyValue`]. No-op
/// for primitive immediates (Int32 / f64 / Bool / Null /
/// Undefined). For cell values, calls
/// [`__torajs_rc_inc`](torajs_rc::__torajs_rc_inc) on the
/// underlying pointer.
///
/// # Safety
///
/// If [`is_cell(v)`], the encoded pointer is null or a valid
/// `*mut HeapHeader`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_rc_inc(v: AnyValue) {
    // Step 8b-C: ShortStr is a primitive immediate — no heap, no rc.
    if is_short_str(v) {
        return;
    }
    if is_cell(v) {
        // SAFETY: is_cell guarantees the pointer is non-zero and
        // 8-aligned; the caller invariant says it points to a
        // valid heap object.
        unsafe { __torajs_rc_inc(as_void_ptr(v)) };
    }
}

/// Release one reference to the heap payload of an [`AnyValue`].
/// No-op for primitive immediates. For cell values, delegates to
/// `__torajs_value_drop_heap`, whose per-type arms rc-dec and free
/// on hit-zero (the same "release one reference" contract every
/// other caller — array slot walks, dynobj entry walks — uses).
///
/// The pre-RFC shape dec'd here FIRST and only then called
/// `value_drop_heap`; the arm's own dec then underflowed (rc 0 →
/// u32::MAX → Keep), so any heap value whose LAST reference was
/// released through the `any` world never freed (RFC 20260704 S6).
///
/// This is the immediate-mode replacement for the legacy
/// box-drop shim deleted in Step 7f-D-1.
///
/// # Safety
///
/// If [`is_cell(v)`], the encoded pointer must point to a valid
/// heap object that the caller owns a reference to.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_rc_dec(v: AnyValue) {
    // Step 8b-C: ShortStr is a primitive immediate — no heap, no rc.
    if is_short_str(v) {
        return;
    }
    if is_cell(v) {
        // SAFETY: as above; the arm dec-gates and frees on hit-zero.
        unsafe { __torajs_value_drop_heap(as_void_ptr(v)) };
    }
}

// ============================================================
// Coercion — direct on immediates, delegate on cells
// ============================================================

/// `ToNumber(v)` per ES §7.1.4. Primitives compute inline; cell
/// values delegate to [`crate::coerce::any_to_number`] with a
/// synthesized `(Heap, ptr)` pair.
///
/// # Safety
///
/// Cell case: encoded pointer must be valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_to_number(v: AnyValue) -> f64 {
    if is_int32(v) {
        return as_int32(v) as f64;
    }
    if is_double(v) {
        return as_double(v);
    }
    if is_null(v) {
        return 0.0;
    }
    if is_undefined(v) {
        return f64::NAN;
    }
    if is_bool(v) {
        return if as_bool(v) { 1.0 } else { 0.0 };
    }
    // Step 8b-C: ShortStr → materialize to Heap+Str + parse via
    // __torajs_str_to_number. Future polish (8d/8e): inline byte
    // parsing avoids the alloc (most short numerics: "0", "1",
    // "true", etc.) but for now match the existing Heap+Str path
    // behavior exactly.
    if is_short_str(v) {
        // SAFETY: ShortStr ≤ 5 byte payload; materialize gives a
        // fresh Heap+Str with refcount=1. str_to_number reads the
        // Str layout. We then drop the temporary Str.
        let s = unsafe { materialize_short_str(v) };
        let n = unsafe { __torajs_str_to_number(s as *const c_void) };
        // SAFETY: s is the freshly-materialized Str we own; rc=1.
        unsafe { drop_materialized_str(s) };
        return n;
    }
    if is_cell(v) {
        // SAFETY: cell pointer is non-null + valid HeapHeader.
        return unsafe { any_to_number(AnySlotTag::Heap as i64, v as i64) };
    }
    f64::NAN
}

/// `String(v)` per ES §22.1.1 — like [`__torajs_anyv_to_str`] except
/// a Symbol answers its SymbolDescriptiveString instead of the
/// §7.1.17 implicit-coercion TypeError (step 1.a: the explicit
/// String() call is the ONE place a symbol stringifies).
///
/// # Safety
/// Cell case: encoded pointer must be valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_to_display_str(v: AnyValue) -> *mut c_void {
    if is_cell(v) {
        let ptr = as_void_ptr(v);
        let tag = unsafe { (ptr.cast::<u8>().add(4) as *const u16).read() };
        if tag == Tag::Symbol as u16 {
            unsafe extern "C" {
                fn __torajs_symbol_to_str(p: *const c_void) -> *mut u8;
            }
            return unsafe { __torajs_symbol_to_str(ptr) as *mut c_void };
        }
    }
    unsafe { __torajs_anyv_to_str(v) }
}

/// `ToString(v)` per ES §7.1.17. Returns a freshly-owned
/// `*mut Str` the caller must drop.
///
/// # Safety
///
/// Cell case: encoded pointer must be valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_to_str(v: AnyValue) -> *mut c_void {
    if is_int32(v) {
        return unsafe { any_to_str(AnySlotTag::I64 as i64, as_int32(v) as i64) };
    }
    if is_double(v) {
        let bits = as_double(v).to_bits() as i64;
        return unsafe { any_to_str(AnySlotTag::F64 as i64, bits) };
    }
    if is_null(v) {
        return unsafe { any_to_str(AnySlotTag::Null as i64, 0) };
    }
    if is_undefined(v) {
        return unsafe { any_to_str(AnySlotTag::Undef as i64, 0) };
    }
    if is_bool(v) {
        return unsafe { any_to_str(AnySlotTag::Bool as i64, if as_bool(v) { 1 } else { 0 }) };
    }
    // Step 8b-C: ShortStr → materialize. ToString contract returns
    // a freshly-owned Str pointer the caller drops; materializing
    // gives exactly that. Future polish (8d/8e): if the caller can
    // accept an AnyValue result instead of *mut c_void, return v
    // directly without alloc.
    if is_short_str(v) {
        return unsafe { materialize_short_str(v) as *mut c_void };
    }
    if is_cell(v) {
        return unsafe { any_to_str(AnySlotTag::Heap as i64, v as i64) };
    }
    // Defensive — unreachable in well-formed runtime.
    unsafe { any_to_str(AnySlotTag::Null as i64, 0) }
}

/// `ToBoolean(v)` per ES §7.1.2. Falsy: `null`, `undefined`,
/// `false`, `+0`, `-0`, `NaN`, `""`. Truthy: everything else.
///
/// # Safety
///
/// Cell case: encoded pointer must point to a valid heap object
/// (Str header read for the empty-string check).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_to_bool(v: AnyValue) -> bool {
    if is_int32(v) {
        return as_int32(v) != 0;
    }
    if is_double(v) {
        let f = as_double(v);
        return !f.is_nan() && f != 0.0;
    }
    if is_null(v) || is_undefined(v) {
        return false;
    }
    if is_bool(v) {
        return as_bool(v);
    }
    // Step 8b-C: ShortStr "" is falsy per ES; non-empty truthy.
    // Cheaper than the Heap+Str path — no heap deref, just read
    // 8 bits of the immediate.
    if is_short_str(v) {
        return short_str_len(v) != 0;
    }
    if is_cell(v) {
        let ptr = as_pointer(v);
        // SAFETY: cell pointer non-null per is_cell guarantee.
        let h = unsafe { &*ptr };
        if matches!(h.tag(), Tag::Str) {
            // Str layout: [header:8][length:4][_pad:4][bytes:N].
            // `length` is a u32; the four bytes above it are the
            // capacity slot and are not part of it.
            let len_ptr = (ptr as *const u8).wrapping_add(8) as *const u32;
            // SAFETY: Tag::Str heap invariant — len field present
            // at offset 8 in the layout.
            return unsafe { *len_ptr != 0 };
        }
        if matches!(h.tag(), Tag::BigInt) {
            // ES §7.1.4 ToBoolean(BigInt) — 0n is falsy. Delegate to
            // the BigInt runtime helper (reads the layout-owned `len`
            // field; len == 0 iff the value is 0n).
            return unsafe {
                crate::loose_eq::bigint_ffi::__torajs_bigint_is_nonzero(ptr as *const c_void)
            } != 0;
        }
        // Other heap objects (Arr, Obj, Closure, ...) → true.
        return true;
    }
    false
}

mod ops;
pub use ops::*;

// ============================================================
// Tests — small coverage of the new shim ABI. Heavy decode/
// encode round-trips live in nanbox.rs; this module focuses on
// the immediate-vs-cell dispatch shape.
// ============================================================

#[cfg(test)]
mod tests {
    use super::ops::decode_to_tag_value;
    use super::*;
    use crate::AnySlotTag;
    use crate::nanbox::{
        VALUE_FALSE, VALUE_NULL, VALUE_TRUE, VALUE_UNDEFINED, box_double, box_int32,
    };
    use crate::nanbox_encode::{__torajs_anyv_null, __torajs_anyv_undefined};

    // Stubs for the C symbols declared in `crate::tests` (top-of-
    // lib.rs `#[cfg(test)] mod tests`) — `__torajs_value_drop_heap`,
    // `__torajs_str_eq`, `__torajs_str_to_number`,
    // `__torajs_weakref_target_dying`. They're crate-wide once
    // linked so we don't redeclare here.

    #[test]
    fn anyv_to_number_primitives() {
        unsafe {
            assert_eq!(__torajs_anyv_to_number(__torajs_anyv_null()), 0.0);
            assert!(__torajs_anyv_to_number(__torajs_anyv_undefined()).is_nan());
            assert_eq!(__torajs_anyv_to_number(VALUE_FALSE), 0.0);
            assert_eq!(__torajs_anyv_to_number(VALUE_TRUE), 1.0);
            assert_eq!(__torajs_anyv_to_number(box_int32(42)), 42.0);
            assert_eq!(__torajs_anyv_to_number(box_int32(-7)), -7.0);
            assert_eq!(__torajs_anyv_to_number(box_double(3.14)), 3.14);
            assert!(__torajs_anyv_to_number(box_double(f64::NAN)).is_nan());
        }
    }

    #[test]
    fn anyv_to_bool_primitives() {
        unsafe {
            assert!(!__torajs_anyv_to_bool(VALUE_NULL));
            assert!(!__torajs_anyv_to_bool(VALUE_UNDEFINED));
            assert!(!__torajs_anyv_to_bool(VALUE_FALSE));
            assert!(__torajs_anyv_to_bool(VALUE_TRUE));
            assert!(!__torajs_anyv_to_bool(box_int32(0)));
            assert!(__torajs_anyv_to_bool(box_int32(1)));
            assert!(__torajs_anyv_to_bool(box_int32(-1)));
            assert!(!__torajs_anyv_to_bool(box_double(0.0)));
            assert!(!__torajs_anyv_to_bool(box_double(-0.0)));
            assert!(!__torajs_anyv_to_bool(box_double(f64::NAN)));
            assert!(__torajs_anyv_to_bool(box_double(3.14)));
            assert!(__torajs_anyv_to_bool(box_double(f64::INFINITY)));
        }
    }

    #[test]
    fn anyv_strict_eq_primitives() {
        unsafe {
            // identity
            assert!(__torajs_anyv_strict_eq(VALUE_NULL, VALUE_NULL));
            assert!(__torajs_anyv_strict_eq(VALUE_UNDEFINED, VALUE_UNDEFINED));
            assert!(__torajs_anyv_strict_eq(VALUE_TRUE, VALUE_TRUE));
            assert!(__torajs_anyv_strict_eq(VALUE_FALSE, VALUE_FALSE));
            assert!(__torajs_anyv_strict_eq(box_int32(42), box_int32(42)));
            assert!(__torajs_anyv_strict_eq(box_double(3.14), box_double(3.14)));

            // distinct
            assert!(!__torajs_anyv_strict_eq(VALUE_NULL, VALUE_UNDEFINED));
            assert!(!__torajs_anyv_strict_eq(VALUE_TRUE, VALUE_FALSE));
            assert!(!__torajs_anyv_strict_eq(box_int32(42), box_int32(43)));

            // NaN exception: NaN !== NaN
            assert!(!__torajs_anyv_strict_eq(
                box_double(f64::NAN),
                box_double(f64::NAN)
            ));

            // ±0: +0 === -0
            assert!(__torajs_anyv_strict_eq(box_double(0.0), box_double(-0.0)));

            // cross-type numeric: 1 === 1.0
            assert!(__torajs_anyv_strict_eq(box_int32(1), box_double(1.0)));
            assert!(__torajs_anyv_strict_eq(box_double(2.0), box_int32(2)));
            assert!(!__torajs_anyv_strict_eq(box_int32(1), box_double(1.5)));
        }
    }

    #[test]
    fn anyv_rc_inc_no_op_on_primitives() {
        // Just verifies no panic / no crash for non-cell values.
        unsafe {
            __torajs_anyv_rc_inc(VALUE_NULL);
            __torajs_anyv_rc_inc(VALUE_UNDEFINED);
            __torajs_anyv_rc_inc(VALUE_TRUE);
            __torajs_anyv_rc_inc(VALUE_FALSE);
            __torajs_anyv_rc_inc(box_int32(42));
            __torajs_anyv_rc_inc(box_double(3.14));
            __torajs_anyv_rc_dec(VALUE_NULL);
            __torajs_anyv_rc_dec(box_int32(42));
        }
    }

    #[test]
    fn decode_to_tag_value_round_trip() {
        // I32 → I64 tag.
        let (t, v) = decode_to_tag_value(box_int32(42));
        assert_eq!(t, AnySlotTag::I64 as i64);
        assert_eq!(v, 42);

        // F64 → F64 tag with bitcast payload.
        let (t, v) = decode_to_tag_value(box_double(3.14));
        assert_eq!(t, AnySlotTag::F64 as i64);
        assert_eq!(f64::from_bits(v as u64), 3.14);

        // Null / Undef / Bool round-trip.
        let (t, _) = decode_to_tag_value(VALUE_NULL);
        assert_eq!(t, AnySlotTag::Null as i64);
        let (t, _) = decode_to_tag_value(VALUE_UNDEFINED);
        assert_eq!(t, AnySlotTag::Undef as i64);
        let (t, v) = decode_to_tag_value(VALUE_TRUE);
        assert_eq!(t, AnySlotTag::Bool as i64);
        assert_eq!(v, 1);
    }

    // ----- ShortStr (Step 8b-C) -----
    //
    // These tests cover the primitive-side dispatch only (no heap
    // materialization). Cross-type ShortStr × Heap+Str behavior is
    // verified by the conformance gate (685/0/1) — that path needs
    // the full Str alloc/free runtime, which is integration-test
    // territory.

    use crate::nanbox::{is_short_str, short_str_len, try_box_short_str};

    #[test]
    fn anyv_rc_inc_dec_short_str_is_noop() {
        // ShortStr is primitive immediate — no heap, no rc tracking.
        // Verify rc_inc / rc_dec do not panic / dereference anything.
        unsafe {
            let v = try_box_short_str(b"abc").unwrap();
            __torajs_anyv_rc_inc(v);
            __torajs_anyv_rc_dec(v);
            // 5-byte boundary
            let v_max = try_box_short_str(b"abcde").unwrap();
            __torajs_anyv_rc_inc(v_max);
            __torajs_anyv_rc_dec(v_max);
            // Empty ShortStr
            let v_empty = try_box_short_str(b"").unwrap();
            __torajs_anyv_rc_inc(v_empty);
            __torajs_anyv_rc_dec(v_empty);
        }
    }

    #[test]
    fn anyv_to_bool_short_str() {
        unsafe {
            // Empty ShortStr is falsy per ES
            let v_empty = try_box_short_str(b"").unwrap();
            assert!(!__torajs_anyv_to_bool(v_empty));

            // Non-empty ShortStr is truthy
            let v_a = try_box_short_str(b"a").unwrap();
            assert!(__torajs_anyv_to_bool(v_a));
            let v_max = try_box_short_str(b"abcde").unwrap();
            assert!(__torajs_anyv_to_bool(v_max));
            // Even "0" is truthy (string, not number 0)
            let v_zero = try_box_short_str(b"0").unwrap();
            assert!(__torajs_anyv_to_bool(v_zero));
        }
    }

    #[test]
    fn anyv_strict_eq_short_str_identity() {
        unsafe {
            // Same bytes encode to identical u64 → identity fast
            // path catches.
            let a = try_box_short_str(b"abc").unwrap();
            let b = try_box_short_str(b"abc").unwrap();
            assert_eq!(a, b, "same bytes must encode identically");
            assert!(__torajs_anyv_strict_eq(a, b));
            // Same-empty
            let e1 = try_box_short_str(b"").unwrap();
            let e2 = try_box_short_str(b"").unwrap();
            assert!(__torajs_anyv_strict_eq(e1, e2));
        }
    }

    #[test]
    fn anyv_strict_eq_short_str_different_bytes_false() {
        unsafe {
            let a = try_box_short_str(b"abc").unwrap();
            let b = try_box_short_str(b"abd").unwrap();
            assert_ne!(a, b);
            assert!(!__torajs_anyv_strict_eq(a, b));
            // Different lengths, same prefix
            let c = try_box_short_str(b"ab").unwrap();
            let d = try_box_short_str(b"abc").unwrap();
            assert!(!__torajs_anyv_strict_eq(c, d));
        }
    }

    #[test]
    fn anyv_strict_eq_short_str_vs_non_string_primitive_false() {
        // ShortStr vs Int32 / f64 / Null / Undef / Bool: all false
        // per ES strict-eq semantics ("a" !== 0 etc.)
        unsafe {
            let s = try_box_short_str(b"abc").unwrap();
            assert!(!__torajs_anyv_strict_eq(s, box_int32(0)));
            assert!(!__torajs_anyv_strict_eq(s, box_int32(42)));
            assert!(!__torajs_anyv_strict_eq(s, box_double(3.14)));
            assert!(!__torajs_anyv_strict_eq(s, VALUE_NULL));
            assert!(!__torajs_anyv_strict_eq(s, VALUE_UNDEFINED));
            assert!(!__torajs_anyv_strict_eq(s, VALUE_TRUE));
            assert!(!__torajs_anyv_strict_eq(s, VALUE_FALSE));
            // And symmetric direction
            assert!(!__torajs_anyv_strict_eq(box_int32(0), s));
            assert!(!__torajs_anyv_strict_eq(VALUE_NULL, s));
        }
    }

    #[test]
    fn short_str_predicate_disjoint_from_decode_to_tag_value_arms() {
        // ShortStr falls through decode_to_tag_value's predicate
        // chain to the `else { Null }` arm — this is documented as
        // UB if called without materialize_if_short. Verify it
        // doesn't accidentally match an earlier arm.
        let v = try_box_short_str(b"abc").unwrap();
        assert!(is_short_str(v));
        let (t, _) = decode_to_tag_value(v);
        // The post-condition is documented: ShortStr without
        // materialize routes to the defensive Null. This isn't a
        // bug — callers (compare/arith/add) materialize first.
        assert_eq!(t, AnySlotTag::Null as i64);
    }

    #[test]
    fn materialize_if_short_passes_through_non_shortstr() {
        // Non-ShortStr inputs return (v, None) — no temporaries.
        let inputs = [
            VALUE_NULL,
            VALUE_UNDEFINED,
            VALUE_TRUE,
            VALUE_FALSE,
            box_int32(42),
            box_double(3.14),
        ];
        for v in inputs {
            // SAFETY: pure inspection — no materialize fires.
            let (out, tmp) = unsafe { materialize_if_short(v) };
            assert_eq!(out, v);
            assert!(tmp.is_none(), "non-ShortStr must not materialize");
        }
    }

    #[test]
    fn short_str_len_extracted_correctly_through_shim() {
        // Sanity: shim-side short_str_len matches nanbox-side.
        for bytes in [&b""[..], b"a", b"ab", b"abc", b"abcd", b"abcde"] {
            let v = try_box_short_str(bytes).unwrap();
            assert_eq!(short_str_len(v) as usize, bytes.len());
        }
    }
}
