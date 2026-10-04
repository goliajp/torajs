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
    // `v == 0` is the array slot HOLE (`FLAG_ARR_ANY_HOLEY`) — read,
    // it is undefined.
    alloc_literal(b"undefined")
}

/// `typeof v === "string"` as an integer — the test the boxed adapter
/// and a direct call's run-time split ask of an `any` argument headed
/// for a `string` parameter. 1 for an inline string or a `Tag::Str`
/// cell; 0 for everything else (`null`, `undefined`, a number, an
/// object), which such a parameter cannot hold: ToString would hand
/// the body `"null"` where §10.2.1.3 binds the argument as passed.
///
/// # Safety
///
/// Cell case: the encoded pointer names a live heap cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_str_fits(v: AnyValue) -> i64 {
    if is_short_str(v) {
        return 1;
    }
    if !is_cell(v) {
        return 0;
    }
    let tag = unsafe { heap_type_tag(as_void_ptr(v) as *const c_void) };
    i64::from(tag == Tag::Str as u16)
}

unsafe extern "C" {
    /// torajs-arr — an array block's fit to a slot shape word.
    fn __torajs_anyv_arr_cell(v: u64, shape: u64) -> i64;
}

/// `__torajs_anyv_rest_fits(argv, from, argc, shape)` — 1 when every
/// argument `argv[from..argc]` fits an element of a typed rest
/// parameter whose array has the shape word `shape`
/// (`torajs_rc::arr_kind::SLOT_LEVEL_BITS`). The adapter asks it ahead
/// of collecting the rest, whose typed conversion would coerce a
/// `null` into `"null"` or throw where §10.2.1.3 binds it as passed;
/// a miss hands the call to the all-`any` twin.
///
/// # Safety
///
/// `argv` holds `argc` valid NaN-boxes; cells they name are live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_rest_fits(
    argv: *const AnyValue,
    from: i64,
    argc: i64,
    shape: u64,
) -> i64 {
    i64::from((from..argc).all(|i| unsafe { elem_fits(*argv.add(i as usize), shape) }))
}

/// Does `v` fit one element of an array level of `shape`? An integer
/// level takes only integral numbers its slots can hold exactly.
///
/// # Safety
///
/// Cell case: the encoded pointer names a live heap cell.
unsafe fn elem_fits(v: AnyValue, shape: u64) -> bool {
    use torajs_rc::arr_kind::{SLOT_LEVEL_BITS, SLOT_TAG_NONE};
    use torajs_rc::{ARR_KIND_BOOL, ARR_KIND_F64, ARR_KIND_HEAP, ARR_KIND_I64, ARR_KIND_UNSET};
    let want = (shape >> 3) & SLOT_TAG_NONE;
    let kind = (shape & 0b111) as u16;
    if kind == ARR_KIND_UNSET {
        true
    } else if kind == ARR_KIND_I64 {
        is_int32(v) || is_double(v) && integral(crate::nanbox::as_double(v))
    } else if kind == ARR_KIND_F64 {
        is_int32(v) || is_double(v)
    } else if kind == ARR_KIND_BOOL {
        is_bool(v)
    } else if kind == ARR_KIND_HEAP && want == Tag::Str as u64 {
        unsafe { __torajs_anyv_str_fits(v) == 1 }
    } else if kind == ARR_KIND_HEAP && want == Tag::Arr as u64 {
        unsafe { __torajs_anyv_arr_cell(v, shape >> SLOT_LEVEL_BITS) == 1 }
    } else {
        false
    }
}

/// An integral number an `i64` slot holds without loss, `-0` excluded.
fn integral(d: f64) -> bool {
    d.fract() == 0.0 && d.abs() <= 9_007_199_254_740_992.0 && (d != 0.0 || d.is_sign_positive())
}
