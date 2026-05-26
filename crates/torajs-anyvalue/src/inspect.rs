//! Any-tag inspection — `typeof box` + `console.log(box)` +
//! `ToBoolean(box)` — port of `runtime_str.c` L795-833 +
//! L1040-1157.
//!
//! Two extern fns that read an [`AnyBox`]'s discriminant and route:
//!
//! - [`__torajs_any_typeof`] — returns a fresh Str holding the ES
//!   `typeof` result for the box (`"object"` / `"undefined"` /
//!   `"boolean"` / `"number"` / `"string"` / `"function"` /
//!   `"symbol"` / `"bigint"`).
//!
//! - [`__torajs_print_any`] — `console.log(box)` dispatch. Routes to
//!   the IR-emitted `print_i64` / `print_f64` / `print_bool` and
//!   `__torajs_str_print` based on the slot tag. ANY_HEAP recurses
//!   through the heap value's universal `type_tag`; only `Str` gets
//!   pretty-printed today, everything else falls back to
//!   `"[object]\n"` (heap-typed pretty-print is a later wedge).
//!
//! Cross-tier symbols resolved at `tr build` link time:
//! - `print_i64` / `print_f64` / `print_bool` — IR-emitted in
//!   ssa_inkwell (per-byte putchar stdio buffer).
//! - `__torajs_str_print` — `libtorajs_str.a`.
//! - `__torajs_str_alloc_pooled` — `libtorajs_str.a`.
//! - `putchar` — libc; per-byte writer shared with the print family.

use core::ffi::c_void;

use crate::nanbox::{
    AnyValue, as_bool, as_double, as_int32, as_void_ptr, is_bool, is_cell, is_double, is_int32,
    is_null, is_undefined,
};
use crate::nanbox_ffi::__torajs_anyv_to_bool;
use torajs_rc::{HeapHeader, Tag};

const STR_HDR_SIZE: usize = 16;

unsafe extern "C" {
    fn __torajs_str_alloc_pooled(len: u64) -> *mut u8;
    fn __torajs_str_print(s: *const u8);
    fn print_i64(n: i64);
    fn print_f64(d: f64);
    fn print_bool(b: bool);
    fn putchar(c: i32) -> i32;
}

#[inline]
fn alloc_literal(s: &[u8]) -> *mut u8 {
    let p = unsafe { __torajs_str_alloc_pooled(s.len() as u64) };
    if !s.is_empty() {
        unsafe { core::ptr::copy_nonoverlapping(s.as_ptr(), p.add(STR_HDR_SIZE), s.len()) };
    }
    p
}

#[inline]
fn write_line(s: &[u8]) {
    for &b in s {
        unsafe { putchar(b as i32) };
    }
}

#[inline]
unsafe fn heap_type_tag(child: *const c_void) -> u16 {
    unsafe { (*(child as *const HeapHeader)).type_tag }
}

/// `typeof box` per ES §13.5.3 — returns a fresh Str. Step 7d-A
/// delegate: forwards to [`__torajs_anyv_typeof`], the NaN-box
/// `AnyValue` entry point. `box_ptr` carries the `AnyValue`
/// bit-pattern (or zero for an uninitialized slot, which decodes
/// as `"object"` per `typeof null === "object"`).
///
/// # Safety
/// `box_ptr` carries an `AnyValue` bit-pattern.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_typeof(box_ptr: *const c_void) -> *mut u8 {
    // SAFETY: caller invariant — propagated.
    unsafe { __torajs_anyv_typeof(box_ptr as AnyValue) }
}

/// `console.log(box)` dispatch — single-arg form. Step 7d-A
/// delegate: forwards to [`__torajs_print_anyv`], the NaN-box
/// entry point. `box_ptr` carries the `AnyValue` bit-pattern.
///
/// Trailing newline matches every other `print_*` helper; multi-
/// arg `console.log` goes through the space-joiner upstream and
/// calls this for each arg in turn.
///
/// # Safety
/// `box_ptr` carries an `AnyValue` bit-pattern.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_print_any(box_ptr: *const c_void) {
    // SAFETY: caller invariant — propagated.
    unsafe { __torajs_print_anyv(box_ptr as AnyValue) };
}

/// `ToBoolean(box)` per ES §7.1.2 — JS truthiness coercion. Step
/// 7d-A delegate: forwards to [`__torajs_anyv_to_bool`], the
/// NaN-box entry point. The full dispatch table (`Null`/`Undef` →
/// `false`, `Bool`/`I64` → `value != 0`, `F64` → ordered-non-zero,
/// `Heap`/`Str` → `length > 0`, other heap → `true`) lives in
/// [`__torajs_anyv_to_bool`].
///
/// # Safety
/// `box_ptr` carries an `AnyValue` bit-pattern.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_to_bool(box_ptr: *const c_void) -> bool {
    // SAFETY: caller invariant — propagated.
    unsafe { __torajs_anyv_to_bool(box_ptr as AnyValue) }
}

/// `typeof v` per ES §13.5.3 — NaN-box [`AnyValue`] entry point
/// (Step 7d). Returns a fresh Str. Mirrors [`__torajs_any_typeof`]
/// but dispatches on the immediate NaN-box predicates instead of
/// reading an AnyBox struct.
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
    if is_cell(v) {
        let child = as_void_ptr(v) as *const c_void;
        // SAFETY: cell pointer is non-null per is_cell guarantee +
        // caller invariant says it points to a live heap object.
        let tag = unsafe { heap_type_tag(child) };
        let kind: &[u8] = if tag == Tag::Str as u16 {
            b"string"
        } else if tag == Tag::Closure as u16 {
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

/// `console.log(v)` single-arg dispatch — NaN-box [`AnyValue`]
/// entry point (Step 7d). Mirrors [`__torajs_print_any`] but
/// dispatches on the immediate NaN-box predicates.
///
/// # Safety
///
/// Cell case: encoded pointer must point to a valid heap object.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_print_anyv(v: AnyValue) {
    if is_null(v) {
        write_line(b"null\n");
        return;
    }
    if is_undefined(v) {
        write_line(b"undefined\n");
        return;
    }
    if is_bool(v) {
        // SAFETY: extern fn callable from this no_std-ish module.
        unsafe { print_bool(as_bool(v)) };
        return;
    }
    if is_int32(v) {
        let n = as_int32(v) as i64;
        // SAFETY: as above.
        unsafe { print_i64(n) };
        return;
    }
    if is_double(v) {
        let d = as_double(v);
        // SAFETY: as above.
        unsafe { print_f64(d) };
        return;
    }
    if is_cell(v) {
        let child = as_void_ptr(v) as *const c_void;
        // SAFETY: live heap ptr per caller invariant.
        let tag = unsafe { heap_type_tag(child) };
        if tag == Tag::Str as u16 {
            // SAFETY: Tag::Str header layout — print walker reads
            // len@+8 and bytes@+16.
            unsafe { __torajs_str_print(child as *const u8) };
        } else {
            write_line(b"[object]\n");
        }
        return;
    }
    write_line(b"[unknown-any-tag]\n");
}
