//! `typeof v` — §13.5.3, split out of `any.rs` by the file-size
//! discipline (RFC 20260823-typedarray-substrate 刀 1 pushed that
//! file to two lines of margin).
//!
//! The seam is one the parent file's own module doc already drew:
//! it named "`console.log(v)` / `typeof v`" as two entries, and they
//! answer two different questions — how a value RENDERS versus what
//! the language CALLS it. Nothing here writes to stdout.

use core::ffi::c_void;

use torajs_rc::Tag;

use super::formatters::{alloc_literal, heap_type_tag};
use crate::nanbox::{
    AnyValue, as_void_ptr, is_bool, is_cell, is_double, is_int32, is_null, is_short_str,
    is_undefined,
};

/// `typeof v` per ES §13.5.3 — NaN-box [`AnyValue`] entry point.
/// Returns a fresh Str. Dispatches on the immediate NaN-box
/// predicates (no heap struct read).
///
/// # Safety
///
/// Cell case: encoded pointer must point to a valid heap object
/// (only the `HeapHeader::type_tag` at +4 is read).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_typeof(v: AnyValue) -> *mut u8 {
    if is_null(v) {
        return alloc_literal(b"object");
    }
    if is_undefined(v) {
        return alloc_literal(b"undefined");
    }
    if is_bool(v) {
        return alloc_literal(b"boolean");
    }
    if is_int32(v) || is_double(v) {
        return alloc_literal(b"number");
    }
    // Step 8c — ShortStr is a string at the JS surface even though
    // its bits live inline in the AnyValue immediate; report
    // `typeof` as `"string"` BEFORE the cell-pointer branch (which
    // would mis-dispatch to `"object"` via the fall-through arm).
    if is_short_str(v) {
        return alloc_literal(b"string");
    }
    if is_cell(v) {
        let child = as_void_ptr(v) as *const c_void;
        // SAFETY: cell pointer is non-null per is_cell guarantee +
        // caller invariant says it points to a live heap object.
        let tag = unsafe { heap_type_tag(child) };
        let kind: &[u8] = if tag == Tag::Str as u16 {
            b"string"
        } else if unsafe { crate::construct::is_callable(v) } {
            // Step 9 IS IsCallable — closures, a Proxy that reads
            // through to a callable target (§10.5.14 step 3), and
            // the `__class_<C>` class-constructor dynobjs alike (ES
            // models class constructors as function objects; RFC
            // 20260717-class-first-class-value knife A). The list of
            // shapes lives in one place so a new callable shape
            // cannot reach `typeof` and miss a brand gate, or the
            // reverse.
            b"function"
        } else if tag == Tag::Symbol as u16 {
            b"symbol"
        } else if tag == Tag::BigInt as u16 {
            b"bigint"
        } else {
            // OBJ / ARR / REGEX / DATE / WEAK* / DYNOBJ / MAP* /
            // ARR_ITER → "object"
            b"object"
        };
        return alloc_literal(kind);
    }
    // Defensive — uninitialized slot (v == 0) reads as "object"
    // (matches `typeof null` per spec).
    alloc_literal(b"object")
}
