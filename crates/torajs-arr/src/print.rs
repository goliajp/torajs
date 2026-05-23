//! `console.log(arr)` pretty-print, per-element-type variants.
//!
//! Port of `runtime_str.c::__torajs_arr_print_{i64,f64,bool,str,substr}`
//! (P4.1-g, 2026-05-23).
//!
//! Output shape (matches bun for these element types):
//! - `undefined\n` for NULL arr
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
//! Uses extern `putchar` per-byte (NOT `std::io::stdout`) to share the C
//! stdio stdout buffer with still-IR-emitted `print_i64` / `print_f64`
//! / `print_bool` (scalar variants). Same rationale + constraint as
//! `torajs-str::print::__torajs_str_print`.
//!
//! ## T-13.5 deque-aware
//!
//! Reads `head_offset` (u32 @ offset 20) and folds into the per-slot
//! address so a shifted deque prints in logical order.

use core::ffi::c_void;

use crate::layout::{ARR_LEN_OFF, ARR_SLOTS_OFF};

const ARR_HEAD_OFF: usize = 20;

// Str layout (mirror torajs-str::layout::{STR_LEN_OFF, STR_DATA_OFF}).
// Duplicated to avoid Layer-3 → Layer-2 sibling Cargo dep; same
// cross-tier extern pattern as torajs-num / torajs-bigint use.
const STR_LEN_OFF: usize = 8;
const STR_DATA_OFF: usize = 16;

// Substr layout (mirror torajs-str's substr module).
const SUBSTR_LEN_OFF: usize = 8;
const SUBSTR_PARENT_OFF: usize = 16;
const SUBSTR_OFFSET_OFF: usize = 24;

unsafe extern "C" {
    fn putchar(c: i32) -> i32;
    // Variadic — Rust requires `...` (c_variadic feature) for full
    // signature; we restrict to the two arities we actually call
    // (one for i64, one for f64, one for the format-only path).
    fn snprintf(buf: *mut u8, size: usize, fmt: *const u8, ...) -> i32;
}

// ============================================================
// Output helpers
// ============================================================

#[inline]
unsafe fn put_byte(b: u8) {
    unsafe {
        putchar(b as i32);
    }
}

#[inline]
unsafe fn put_bytes(s: &[u8]) {
    for &b in s {
        unsafe { put_byte(b) };
    }
}

/// Emit `[ ` prefix.
#[inline]
unsafe fn put_open_bracket() {
    unsafe { put_bytes(b"[ ") };
}

/// Emit ` ]\n` suffix.
#[inline]
unsafe fn put_close_bracket() {
    unsafe { put_bytes(b" ]\n") };
}

/// Emit `, ` separator before non-first element.
#[inline]
unsafe fn put_sep(i: u64) {
    if i > 0 {
        unsafe { put_bytes(b", ") };
    }
}

/// Common entry: NULL → "undefined\n", empty → "[]\n", else open bracket
/// + return (head, len) for the caller to drive its per-element loop.
/// Returns `Some((head, len))` when caller should proceed; `None` when
/// already handled the NULL / empty case.
unsafe fn print_header(arr: *const u8) -> Option<(u32, u64)> {
    if arr.is_null() {
        unsafe { put_bytes(b"undefined\n") };
        return None;
    }
    let len = unsafe { *(arr.add(ARR_LEN_OFF) as *const u64) };
    if len == 0 {
        unsafe { put_bytes(b"[]\n") };
        return None;
    }
    let head = unsafe { *(arr.add(ARR_HEAD_OFF) as *const u32) };
    unsafe { put_open_bracket() };
    Some((head, len))
}

#[inline]
unsafe fn slot_addr(arr: *const u8, head: u32, i: u64) -> *const u8 {
    unsafe { arr.add(ARR_SLOTS_OFF + (head as usize + i as usize) * 8) }
}

/// snprintf-format `v` into a stack buffer + emit bytes via putchar.
/// Cap at 64 bytes (any IEEE-754 f64 / i64 print fits comfortably).
unsafe fn put_snprintf_i64(v: i64) {
    let mut buf = [0u8; 64];
    let n = unsafe {
        snprintf(
            buf.as_mut_ptr(),
            64,
            b"%lld\0".as_ptr(),
            v as core::ffi::c_longlong,
        )
    };
    if n > 0 {
        let n = (n as usize).min(63);
        unsafe { put_bytes(&buf[..n]) };
    }
}

unsafe fn put_snprintf_f64_g(v: f64) {
    let mut buf = [0u8; 64];
    let n = unsafe { snprintf(buf.as_mut_ptr(), 64, b"%g\0".as_ptr(), v) };
    if n > 0 {
        let n = (n as usize).min(63);
        unsafe { put_bytes(&buf[..n]) };
    }
}

// ============================================================
// Per-element-type printers
// ============================================================

/// `console.log(arr: Array<I64>)`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_print_i64(arr: *const c_void) {
    let arr = arr as *const u8;
    unsafe {
        let Some((head, len)) = print_header(arr) else {
            return;
        };
        for i in 0..len {
            put_sep(i);
            let v = *(slot_addr(arr, head, i) as *const i64);
            put_snprintf_i64(v);
        }
        put_close_bracket();
    }
}

/// `console.log(arr: Array<F64>)`. JS-spec NaN / Infinity / -Infinity
/// special cases, else `%g` short-form via snprintf (matches C 1:1).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_print_f64(arr: *const c_void) {
    let arr = arr as *const u8;
    unsafe {
        let Some((head, len)) = print_header(arr) else {
            return;
        };
        for i in 0..len {
            put_sep(i);
            let v = *(slot_addr(arr, head, i) as *const f64);
            if v.is_nan() {
                put_bytes(b"NaN");
            } else if v == f64::INFINITY {
                put_bytes(b"Infinity");
            } else if v == f64::NEG_INFINITY {
                put_bytes(b"-Infinity");
            } else {
                put_snprintf_f64_g(v);
            }
        }
        put_close_bracket();
    }
}

/// `console.log(arr: Array<Bool>)`. Slots are i64 (0 / non-0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_print_bool(arr: *const c_void) {
    let arr = arr as *const u8;
    unsafe {
        let Some((head, len)) = print_header(arr) else {
            return;
        };
        for i in 0..len {
            put_sep(i);
            let v = *(slot_addr(arr, head, i) as *const i64);
            put_bytes(if v != 0 { b"true" } else { b"false" });
        }
        put_close_bracket();
    }
}

/// `console.log(arr: Array<Str>)`. Each slot is a `*Str` (NULL → `""`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_print_str(arr: *const c_void) {
    let arr = arr as *const u8;
    unsafe {
        let Some((head, len)) = print_header(arr) else {
            return;
        };
        for i in 0..len {
            put_sep(i);
            let s = *(slot_addr(arr, head, i) as *const *const u8);
            if s.is_null() {
                put_bytes(b"\"\"");
                continue;
            }
            let slen = *(s.add(STR_LEN_OFF) as *const u64);
            put_byte(b'"');
            if slen > 0 {
                let bytes = core::slice::from_raw_parts(s.add(STR_DATA_OFF), slen as usize);
                put_bytes(bytes);
            }
            put_byte(b'"');
        }
        put_close_bracket();
    }
}

/// `console.log(arr: Array<Substr>)`. Each slot is a `*Substr` —
/// layout differs from Str (has parent + offset instead of inline
/// bytes); without this dispatch the bytes would print as garbage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_print_substr(arr: *const c_void) {
    let arr = arr as *const u8;
    unsafe {
        let Some((head, len)) = print_header(arr) else {
            return;
        };
        for i in 0..len {
            put_sep(i);
            let v = *(slot_addr(arr, head, i) as *const *const u8);
            if v.is_null() {
                put_bytes(b"\"\"");
                continue;
            }
            let slen = *(v.add(SUBSTR_LEN_OFF) as *const u64);
            let parent = *(v.add(SUBSTR_PARENT_OFF) as *const *const u8);
            let offset = *(v.add(SUBSTR_OFFSET_OFF) as *const u64);
            put_byte(b'"');
            if slen > 0 {
                let bytes = core::slice::from_raw_parts(
                    parent.add(STR_DATA_OFF + offset as usize),
                    slen as usize,
                );
                put_bytes(bytes);
            }
            put_byte(b'"');
        }
        put_close_bracket();
    }
}
