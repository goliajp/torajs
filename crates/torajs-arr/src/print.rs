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
//! Uses `torajs_io::__torajs_io_putc_stdout` per-byte (v0.7-A3 Step 14-b cutover from libc `putchar`) to share the
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

// Str layout (mirror torajs-str::layout::{STR_LEN_OFF, STR_DATA_OFF,
// STR_FLAG_IS_LATIN1}). Duplicated to avoid Layer-3 → Layer-2
// sibling Cargo dep; same cross-tier extern pattern as torajs-num /
// torajs-bigint use.
const STR_LEN_OFF: usize = 8;
const STR_DATA_OFF: usize = 16;
const STR_FLAG_IS_LATIN1: u16 = 0x0002;
const HDR_FLAGS_OFF: usize = 6;

// Substr layout (mirror torajs-str's substr module).
const SUBSTR_LEN_OFF: usize = 8;
const SUBSTR_PARENT_OFF: usize = 16;
const SUBSTR_OFFSET_OFF: usize = 24;

// ============================================================
// Str payload transcoding helpers (P11.1-S2.1)
// ============================================================

/// Write a single Unicode codepoint as UTF-8 byte sequence via
/// putchar.
#[inline]
unsafe fn put_codepoint_utf8(cp: u32) {
    unsafe {
        if cp <= 0x7F {
            put_byte(cp as u8);
        } else if cp <= 0x7FF {
            put_byte((0xC0 | (cp >> 6)) as u8);
            put_byte((0x80 | (cp & 0x3F)) as u8);
        } else if cp <= 0xFFFF {
            put_byte((0xE0 | (cp >> 12)) as u8);
            put_byte((0x80 | ((cp >> 6) & 0x3F)) as u8);
            put_byte((0x80 | (cp & 0x3F)) as u8);
        } else {
            put_byte((0xF0 | (cp >> 18)) as u8);
            put_byte((0x80 | ((cp >> 12) & 0x3F)) as u8);
            put_byte((0x80 | ((cp >> 6) & 0x3F)) as u8);
            put_byte((0x80 | (cp & 0x3F)) as u8);
        }
    }
}

/// Emit `payload` bytes for an encoded Str: Latin-1 byte ≤ 0x7F
/// passes through verbatim; Latin-1 supplement (0x80..=0xFF)
/// expands to 2-byte UTF-8; UTF-16 LE decodes with surrogate pair
/// handling and re-encodes.
#[inline]
unsafe fn put_str_payload(payload: &[u8], is_latin1: bool) {
    unsafe {
        if is_latin1 {
            for &b in payload {
                if b <= 0x7F {
                    put_byte(b);
                } else {
                    put_byte(0xC0 | (b >> 6));
                    put_byte(0x80 | (b & 0x3F));
                }
            }
            return;
        }
        let mut i = 0usize;
        while i + 1 < payload.len() {
            let cu = u16::from_le_bytes([payload[i], payload[i + 1]]) as u32;
            let cp = if (0xD800..=0xDBFF).contains(&cu) && i + 3 < payload.len() {
                let lo = u16::from_le_bytes([payload[i + 2], payload[i + 3]]) as u32;
                if (0xDC00..=0xDFFF).contains(&lo) {
                    i += 4;
                    0x10000 + ((cu - 0xD800) << 10) + (lo - 0xDC00)
                } else {
                    i += 2;
                    cu
                }
            } else {
                i += 2;
                cu
            };
            put_codepoint_utf8(cp);
        }
    }
}

unsafe extern "C" {
    fn __torajs_io_putc_stdout(c: i32) -> i32;
    // v0.7-A4 Step 15-d: 0-libc i64 + f64 → decimal via
    // torajs-fmt. Replaces libc snprintf "%lld" / "%g" for the
    // arr_print_* element format paths.
    fn __torajs_fmt_itoa(n: i64, out_buf: *mut u8, out_cap: usize) -> i32;
    fn __torajs_fmt_dtoa(d: f64, out_buf: *mut u8, out_cap: usize) -> i32;
}

// ============================================================
// Output helpers
// ============================================================

#[inline]
unsafe fn put_byte(b: u8) {
    unsafe {
        __torajs_io_putc_stdout(b as i32);
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

/// v0.7-A4 Step 15-d: format `v` via `__torajs_fmt_itoa`
/// (0-libc) into a stack buffer + emit bytes via
/// `__torajs_io_putc_stdout`. Replaces libc snprintf("%lld").
unsafe fn put_snprintf_i64(v: i64) {
    let mut buf = [0u8; 64];
    let n = unsafe { __torajs_fmt_itoa(v, buf.as_mut_ptr(), 64) };
    if n > 0 {
        let n = (n as usize).min(63);
        unsafe { put_bytes(&buf[..n]) };
    }
}

/// v0.7-A4 Step 15-d: format `v` via `__torajs_fmt_dtoa`
/// (0-libc; shortest-roundtrip JS-spec shape). Replaces libc
/// snprintf("%g") which truncated to 6 significant digits — the
/// new path matches v8/JSC shortest-roundtrip exactly.
unsafe fn put_snprintf_f64_g(v: f64) {
    let mut buf = [0u8; 64];
    let n = unsafe { __torajs_fmt_dtoa(v, buf.as_mut_ptr(), 64) };
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
///
/// P11.1-S2.1 — reads the Str's `length u32 @8` (was u64 pre-S1)
/// and its `IS_LATIN1` flag bit on `flags u16 @6`. Latin-1 ASCII
/// payloads pass through verbatim; Latin-1 supplement and UTF-16
/// LE payloads transcode to UTF-8 before stdout writes so array
/// elements render correctly regardless of encoding.
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
            let length = *(s.add(STR_LEN_OFF) as *const u32);
            let flags = *(s.add(HDR_FLAGS_OFF) as *const u16);
            let is_latin1 = (flags & STR_FLAG_IS_LATIN1) != 0;
            let byte_cnt = if is_latin1 {
                length as usize
            } else {
                (length as usize) * 2
            };
            put_byte(b'"');
            if byte_cnt > 0 {
                let bytes = core::slice::from_raw_parts(s.add(STR_DATA_OFF), byte_cnt);
                put_str_payload(bytes, is_latin1);
            }
            put_byte(b'"');
        }
        put_close_bracket();
    }
}

/// `console.log(arr: Array<Substr>)`. Each slot is a `*Substr` —
/// layout differs from Str (has parent + offset instead of inline
/// bytes); without this dispatch the bytes would print as garbage.
///
/// P11.1-S2.1 — Substr layout still stores a `u64` byte length
/// (its code-unit flip is queued for S5), but the parent Str's
/// header carries the encoding flag. Transcode parent bytes
/// view via the same Latin-1 / UTF-16 helper so non-ASCII Substrs
/// render correctly.
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
            let slen = *(v.add(SUBSTR_LEN_OFF) as *const u64) as usize;
            let parent = *(v.add(SUBSTR_PARENT_OFF) as *const *const u8);
            let offset = *(v.add(SUBSTR_OFFSET_OFF) as *const u64) as usize;
            put_byte(b'"');
            if slen > 0 {
                let flags = *(parent.add(HDR_FLAGS_OFF) as *const u16);
                let is_latin1 = (flags & STR_FLAG_IS_LATIN1) != 0;
                let bytes = core::slice::from_raw_parts(parent.add(STR_DATA_OFF + offset), slen);
                put_str_payload(bytes, is_latin1);
            }
            put_byte(b'"');
        }
        put_close_bracket();
    }
}
