//! Any-tag inspection — `typeof v` + `console.log(v)` +
//! `ToBoolean(v)` on NaN-box [`AnyValue`] — port of
//! `runtime_str.c` L795-833 + L1040-1157.
//!
//! Two extern fns that read an [`AnyValue`]'s NaN-box discriminant
//! and route:
//!
//! - [`__torajs_anyv_typeof`] — returns a fresh Str holding the ES
//!   `typeof` result for the value (`"object"` / `"undefined"` /
//!   `"boolean"` / `"number"` / `"string"` / `"function"` /
//!   `"symbol"` / `"bigint"`).
//!
//! - [`__torajs_print_anyv`] — `console.log(v)` dispatch. Routes to
//!   the IR-emitted `print_i64` / `print_f64` / `print_bool` and
//!   `__torajs_str_print` based on the NaN-box predicate. Cell case
//!   reads the heap header's universal `type_tag`; only `Str` gets
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
    is_null, is_short_str, is_undefined, short_str_bytes, short_str_len,
};
use torajs_rc::{HeapHeader, Tag};

const STR_HDR_SIZE: usize = 16;

unsafe extern "C" {
    fn __torajs_str_alloc_pooled(len: u64) -> *mut u8;
    fn __torajs_str_print(s: *const u8);
    fn print_i64(n: i64);
    fn print_f64(d: f64);
    fn print_bool(b: bool);
    fn __torajs_io_putc_stdout(c: i32) -> i32;
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
        unsafe { __torajs_io_putc_stdout(b as i32) };
    }
}

#[inline]
unsafe fn heap_type_tag(child: *const c_void) -> u16 {
    unsafe { (*(child as *const HeapHeader)).type_tag }
}

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
/// entry point. Dispatches on the immediate NaN-box predicates.
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
    // Step 8c — ShortStr inline-print path. No heap alloc: read the
    // 8-bit length + 5-byte payload off the immediate and dump
    // bytes through `putchar`. Mirrors how `__torajs_str_print`
    // emits Heap+Str bytes, but skips the materialize round-trip
    // entirely (Heap+Str path goes through __torajs_str_print
    // below).
    if is_short_str(v) {
        let len = short_str_len(v) as usize;
        let bytes = short_str_bytes(v);
        for &b in &bytes[..len] {
            // SAFETY: __torajs_io_putc_stdout takes any i32 byte value.
            unsafe { __torajs_io_putc_stdout(b as i32) };
        }
        unsafe { __torajs_io_putc_stdout(b'\n' as i32) };
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
