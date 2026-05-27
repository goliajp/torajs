//! NaN-box FFI shims — new `__torajs_anyv_*` ABI for the
//! immediate `AnyValue` type (Step 7b).
//!
//! Parallel ABI to the existing `__torajs_any_*` boxed shims in
//! [`crate::ffi`]. ssa_lower / ssa_inkwell callsites continue to
//! emit boxed calls during 7b; the migration to these new shims
//! happens in commits 7c-7f (per
//! [`docs/v0.7-Phase3-nanbox.md`](../../docs/v0.7-Phase3-nanbox.md)).
//!
//! Implementation strategy at 7b:
//!
//! - **Primitive paths** (Int32 / f64 / Bool / Null / Undefined)
//!   run logic directly on the immediate — zero allocation.
//! - **Cell path** decodes the pointer + dispatches to the
//!   existing tag-table inner helpers (`any_to_number`,
//!   `any_to_str`, `any_compare`, `any_arith`, `any_add`) by
//!   synthesizing a `(Heap, ptr)` pair. The inner helpers stay in
//!   their tag-table shape during 7b; 7c-7e rewrite them against
//!   the AnyValue immediate.
//! - **Boxed-result helpers** (`any_arith` / `any_add` return
//!   `*AnyBox`) are unboxed to immediate via
//!   [`box_to_immediate`] then dropped. This burns one AnyBox
//!   alloc per call — eliminated by 7c-7e's helper rewrite.

use std::ffi::c_void;
use std::ptr::NonNull;

use torajs_rc::{__torajs_rc_dec, __torajs_rc_inc, AnySlotTag, HeapHeader, Tag};

use crate::arith::{any_add, any_arith};
use crate::coerce::{any_to_number, any_to_str};
use crate::compare::any_compare;
use crate::nanbox::{
    AnyValue, VALUE_FALSE, VALUE_NULL, VALUE_TRUE, VALUE_UNDEFINED, as_bool, as_double, as_int32,
    as_pointer, as_void_ptr, box_double, box_int32, box_pointer, is_bool, is_cell, is_double,
    is_int32, is_null, is_undefined,
};
use crate::{AnyBox, AnyView};

// ============================================================
// External C symbols re-declared here so the shims don't depend
// on the lib.rs-private extern block.
// ============================================================

unsafe extern "C" {
    fn __torajs_str_eq(a: *const u8, b: *const u8) -> i64;
    fn __torajs_value_drop_heap(child: *mut c_void);
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
    if is_cell(v) {
        // SAFETY: is_cell guarantees the pointer is non-zero and
        // 8-aligned; the caller invariant says it points to a
        // valid heap object.
        unsafe { __torajs_rc_inc(as_void_ptr(v)) };
    }
}

/// Refcount-decrement the heap payload of an [`AnyValue`]. No-op
/// for primitive immediates. For cell values, decrements rc and
/// — if rc transitions to zero — dispatches to
/// `__torajs_value_drop_heap` (the C-side per-type drop walker)
/// to free the heap object.
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
    if is_cell(v) {
        // SAFETY: as above.
        let dec = unsafe { __torajs_rc_dec(as_void_ptr(v)) };
        if dec != 0 {
            // SAFETY: rc transitioned to zero — caller exclusively
            // owns the underlying allocation, safe to drop.
            unsafe { __torajs_value_drop_heap(as_void_ptr(v)) };
        }
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
    if is_cell(v) {
        // SAFETY: cell pointer is non-null + valid HeapHeader.
        return unsafe { any_to_number(AnySlotTag::Heap as i64, v as i64) };
    }
    f64::NAN
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
    if is_cell(v) {
        let ptr = as_pointer(v);
        // SAFETY: cell pointer non-null per is_cell guarantee.
        let h = unsafe { &*ptr };
        if matches!(h.tag(), Tag::Str) {
            // Str layout: [header:8][len:8][bytes:N]. Len at +8.
            let len_ptr = (ptr as *const u8).wrapping_add(8) as *const u64;
            // SAFETY: Tag::Str heap invariant — len field present
            // at offset 8 in the layout.
            return unsafe { *len_ptr != 0 };
        }
        // Other heap objects (Arr, Obj, Closure, ...) → true.
        return true;
    }
    false
}

// ============================================================
// Strict equality
// ============================================================

/// Strict `===` per ES §7.2.13 over [`AnyValue`] operands.
///
/// Identity match handles primitives + heap-pointer identity in
/// one cmp; only the cell-string-equality path needs heap reads.
///
/// JS-spec quirks:
/// - `NaN === NaN` is `false` (even when bits identical).
/// - `+0 === -0` is `true` (they have different bits but compare
///   equal under f64 `==`).
/// - `1 === 1.0` is `true` (cross-type numeric coerces).
///
/// # Safety
///
/// Cell-case operands must point to valid heap objects.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_strict_eq(l: AnyValue, r: AnyValue) -> bool {
    // Identity fast path — bit-equal values are usually equal.
    if l == r {
        // NaN exception: bit-equal NaN compares unequal in JS.
        if is_double(l) {
            return !as_double(l).is_nan();
        }
        return true;
    }
    // f64(0.0) vs f64(-0.0): different bits, equal in JS.
    if is_double(l) && is_double(r) {
        return as_double(l) == as_double(r);
    }
    // Cross-type numeric: i32 vs f64 → coerce both to f64.
    if is_int32(l) && is_double(r) {
        return (as_int32(l) as f64) == as_double(r);
    }
    if is_double(l) && is_int32(r) {
        return as_double(l) == (as_int32(r) as f64);
    }
    // Cell pair: Str↔Str does byte-equality via the C-side
    // __torajs_str_eq (matches the existing AnyBox path).
    if is_cell(l) && is_cell(r) {
        let lp = as_pointer(l);
        let rp = as_pointer(r);
        // SAFETY: both cells non-null per is_cell.
        let (lh, rh) = unsafe { (&*lp, &*rp) };
        if matches!(lh.tag(), Tag::Str) && matches!(rh.tag(), Tag::Str) {
            // SAFETY: both heap headers are Tag::Str — __torajs_str_eq
            // reads the matching Str layout.
            return unsafe { __torajs_str_eq(lp as *const u8, rp as *const u8) != 0 };
        }
        // Other heap pairs: pointer identity only — already
        // covered by `l == r` early-exit above.
        return false;
    }
    false
}

// ============================================================
// Compare / arithmetic — 7b delegates to boxed inner helpers
//
// The inner helpers (any_compare / any_arith / any_add) still
// take `(tag, value)` pairs. 7b decodes the AnyValue immediates
// back to that shape, calls into them, and re-encodes the result.
// 7c-7e replace the inner helpers with AnyValue-direct logic
// (zero box alloc); these decode/re-encode helpers then go away.
// ============================================================

/// Relational compare (`<`, `<=`, `>`, `>=`) per ES §7.2.13.
/// `op`: 0=Lt, 1=Le, 2=Gt, 3=Ge.
///
/// # Safety
///
/// Cell-case operands must point to valid heap objects.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_compare(op: i64, l: AnyValue, r: AnyValue) -> bool {
    let (lt, lv) = decode_to_tag_value(l);
    let (rt, rv) = decode_to_tag_value(r);
    // SAFETY: decode_to_tag_value preserves the cell-pointer
    // validity; any_compare is documented to handle the
    // `(Heap, valid_ptr)` case.
    unsafe { any_compare(op, lt, lv, rt, rv) }
}

/// Arithmetic dispatch (`-`, `*`, `/`, `%`) per ES §13.6-§13.9.
/// `op`: 0=Sub, 1=Mul, 2=Div, 3=Mod. Returns a fresh [`AnyValue`].
///
/// # Safety
///
/// Cell-case operands must point to valid heap objects.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_arith(op: i64, l: AnyValue, r: AnyValue) -> AnyValue {
    let (lt, lv) = decode_to_tag_value(l);
    let (rt, rv) = decode_to_tag_value(r);
    // SAFETY: as above.
    let box_ptr = unsafe { any_arith(op, lt, lv, rt, rv) };
    let result = box_to_immediate(box_ptr);
    // Drop the transitional AnyBox — its content is now in the
    // immediate result. 7c-7e eliminates this round-trip.
    if let Some(p) = NonNull::new(box_ptr as *mut AnyBox) {
        // SAFETY: any_arith returned an owned (refcount = 1)
        // AnyBox; we exclusively own it.
        unsafe { AnyBox::drop_owned(p) };
    }
    result
}

/// `+` per ES §13.15.3 — numeric addition or string concat.
/// Returns a fresh [`AnyValue`].
///
/// # Safety
///
/// Cell-case operands must point to valid heap objects.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_add(l: AnyValue, r: AnyValue) -> AnyValue {
    let (lt, lv) = decode_to_tag_value(l);
    let (rt, rv) = decode_to_tag_value(r);
    // SAFETY: as above.
    let box_ptr = unsafe { any_add(lt, lv, rt, rv) };
    let result = box_to_immediate(box_ptr);
    if let Some(p) = NonNull::new(box_ptr as *mut AnyBox) {
        // SAFETY: same ownership invariant as any_arith.
        unsafe { AnyBox::drop_owned(p) };
    }
    result
}

// ============================================================
// Internal decode/encode bridges
// ============================================================

/// Decode an [`AnyValue`] immediate to the legacy `(tag, value)`
/// pair shape that the un-rewritten inner helpers consume.
/// Heap-pointer values pass through with `tag = Heap` so the
/// helper's `Heap` branch is reached.
fn decode_to_tag_value(v: AnyValue) -> (i64, i64) {
    if is_int32(v) {
        (AnySlotTag::I64 as i64, as_int32(v) as i64)
    } else if is_double(v) {
        (AnySlotTag::F64 as i64, as_double(v).to_bits() as i64)
    } else if is_null(v) {
        (AnySlotTag::Null as i64, 0)
    } else if is_undefined(v) {
        (AnySlotTag::Undef as i64, 0)
    } else if is_bool(v) {
        (AnySlotTag::Bool as i64, if as_bool(v) { 1 } else { 0 })
    } else if is_cell(v) {
        (AnySlotTag::Heap as i64, v as i64)
    } else {
        (AnySlotTag::Null as i64, 0)
    }
}

/// Convert a boxed AnyBox result (returned by `any_arith` /
/// `any_add`) into an immediate [`AnyValue`]. The caller is
/// expected to drop the box after this read.
///
/// Heap payloads have their refcount bumped so the caller of the
/// shim gets an owning ref distinct from the soon-to-be-dropped
/// box's ref.
pub(crate) fn box_to_immediate(box_ptr: *mut c_void) -> AnyValue {
    if box_ptr.is_null() {
        return VALUE_NULL;
    }
    // SAFETY: caller invariant — non-null box pointer.
    let b = unsafe { &*(box_ptr as *const AnyBox) };
    match b.read() {
        AnyView::Null => VALUE_NULL,
        AnyView::Undef => VALUE_UNDEFINED,
        AnyView::Bool(true) => VALUE_TRUE,
        AnyView::Bool(false) => VALUE_FALSE,
        AnyView::I64(n) => {
            if let Ok(n32) = i32::try_from(n) {
                box_int32(n32)
            } else {
                box_double(n as f64)
            }
        }
        AnyView::F64(f) => box_double(f),
        AnyView::Heap(None) => VALUE_NULL,
        AnyView::Heap(Some(p)) => {
            // The box owns one ref to the heap payload and is
            // about to be dropped — bump rc to keep the payload
            // alive for the caller.
            // SAFETY: payload is non-null + valid HeapHeader.
            unsafe { __torajs_rc_inc(p.as_ptr() as *mut c_void) };
            box_pointer(p.as_ptr() as *mut HeapHeader)
        }
        AnyView::Unknown => VALUE_NULL,
    }
}

// ============================================================
// Tests — small coverage of the new shim ABI. Heavy decode/
// encode round-trips live in nanbox.rs; this module focuses on
// the immediate-vs-cell dispatch shape.
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AnySlotTag;
    use crate::nanbox::{box_double, box_int32};
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

    #[test]
    fn box_to_immediate_null_ptr() {
        let v = box_to_immediate(std::ptr::null_mut());
        assert_eq!(v, VALUE_NULL);
    }
}
