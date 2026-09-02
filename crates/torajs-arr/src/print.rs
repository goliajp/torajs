//! `console.log(arr)` pretty-print, per-element-type variants.
//!
//! Port of `runtime_str.c::__torajs_arr_print_{i64,f64,bool,str,substr}`
//! (P4.1-g, 2026-05-23).
//!
//! Output shape (matches bun for these element types):
//! - `null\n` for NULL arr (regex no-match result)
//! - `[]\n` for empty arr
//! - `[ a, b, c ]\n` for non-empty (note the spaces)
//!
//! Element format:
//! - `i64`    — `%lld` (snprintf via extern, byte-equal to C)
//! - `f64`    — JS-spec NaN/Infinity/-Infinity special cases, else `%g`
//!              via snprintf
//! - `bool`   — `true` / `false` (i64 0 vs non-0)
//! - `str`    — `"..."` (Str layout: len@8, bytes@16)
//! - `substr` — `"..."` (Substr layout: len@8, parent_ptr@16, offset@24)
//!
//! ## Buffer-sharing constraint
//!
//! Uses `torajs_io::__torajs_io_putc_out` per-byte (v0.7-A3 Step 14-b cutover from libc `putchar`) to share the
//! stdio stdout buffer with still-IR-emitted `print_i64` / `print_f64`
//! / `print_bool` (scalar variants). Same rationale + constraint as
//! `torajs-str::print::__torajs_str_print`.
//!
//! ## T-13.5 deque-aware
//!
//! Reads `head_offset` (u32 @ offset 20) and folds into the per-slot
//! address so a shifted deque prints in logical order.

use core::ffi::c_void;

use crate::layout::arr_data;
use crate::print_typed::{TypedKind, print_typed_top};

pub(crate) const ARR_HEAD_OFF: usize = 20;

unsafe extern "C" {
    fn __torajs_io_putc_out(c: i32) -> i32;
    // v0.7-A4 Step 15-d: 0-libc i64 + f64 → decimal via
    // torajs-fmt. Replaces libc snprintf "%lld" / "%g" for the
    // arr_print_* element format paths.
    fn __torajs_fmt_itoa(n: i64, out_buf: *mut u8, out_cap: usize) -> i32;
    fn __torajs_fmt_dtoa(d: f64, out_buf: *mut u8, out_cap: usize) -> i32;
    // Line-width estimate accounting (inspect wrap trunk) — hosted
    // in torajs-anyvalue::inspect::formatters.
    fn __torajs_inspect_line_add(n: u32);
}

// ============================================================
// Output helpers
// ============================================================

#[inline]
pub(crate) unsafe fn put_byte(b: u8) {
    unsafe {
        __torajs_io_putc_out(b as i32);
    }
}

#[inline]
pub(crate) unsafe fn put_bytes(s: &[u8]) {
    for &b in s {
        unsafe { put_byte(b) };
    }
}

#[inline]
pub(crate) unsafe fn slot_addr(arr: *const u8, head: u32, i: u64) -> *const u8 {
    unsafe { arr_data(arr).add((head as usize + i as usize) * 8) }
}

/// v0.7-A4 Step 15-d: format `v` via `__torajs_fmt_itoa`
/// (0-libc) into a stack buffer + emit bytes via
/// `__torajs_io_putc_out`. Replaces libc snprintf("%lld").
pub(crate) unsafe fn put_snprintf_i64(v: i64) {
    let mut buf = [0u8; 64];
    let n = unsafe { __torajs_fmt_itoa(v, buf.as_mut_ptr(), 64) };
    if n > 0 {
        let n = (n as usize).min(63);
        unsafe { put_bytes(&buf[..n]) };
        unsafe { __torajs_inspect_line_add(n as u32) };
    }
}

/// v0.7-A4 Step 15-d: format `v` via `__torajs_fmt_dtoa`
/// (0-libc; shortest-roundtrip JS-spec shape). Replaces libc
/// snprintf("%g") which truncated to 6 significant digits — the
/// new path matches v8/JSC shortest-roundtrip exactly.
pub(crate) unsafe fn put_snprintf_f64_g(v: f64) {
    let mut buf = [0u8; 64];
    let n = unsafe { __torajs_fmt_dtoa(v, buf.as_mut_ptr(), 64) };
    if n > 0 {
        let n = (n as usize).min(63);
        unsafe { put_bytes(&buf[..n]) };
        unsafe { __torajs_inspect_line_add(n as u32) };
    }
}

// ============================================================
// Per-element-type printers — thin delegates over the shared
// break/wrap walker (inspect wrap trunk chunk C). Element format
// docs live on `print_typed::emit_elem`.
// ============================================================

/// `console.log(arr: Array<I64>)`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_print_i64(arr: *const c_void) {
    unsafe { print_typed_top(arr, TypedKind::I64) };
    unsafe { put_byte(b'\n') };
}

/// `console.log(arr: Array<F64>)`. JS-spec NaN / Infinity / -Infinity
/// special cases, else shortest-roundtrip dtoa.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_print_f64(arr: *const c_void) {
    unsafe { print_typed_top(arr, TypedKind::F64) };
    unsafe { put_byte(b'\n') };
}

/// `console.log(arr: Array<Bool>)`. Slots are i64 (0 / non-0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_print_bool(arr: *const c_void) {
    unsafe { print_typed_top(arr, TypedKind::Bool) };
    unsafe { put_byte(b'\n') };
}

/// `console.log(arr: Array<Str>)`. Each slot is a `*Str` (NULL →
/// `undefined` — non-participating regex capture).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_print_str(arr: *const c_void) {
    unsafe { print_typed_top(arr, TypedKind::Str) };
    unsafe { put_byte(b'\n') };
}

/// `console.log(arr: Array<Substr>)`. Each slot is a `*Substr` —
/// layout differs from Str (parent + offset instead of inline
/// bytes); without this dispatch the bytes would print as garbage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_print_substr(arr: *const c_void) {
    unsafe { print_typed_top(arr, TypedKind::Substr) };
    unsafe { put_byte(b'\n') };
}
