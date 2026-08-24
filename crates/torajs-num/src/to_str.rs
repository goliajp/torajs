//! Number-to-Str coercions + JS-spec f64 console formatter — port
//! of `runtime_str.c` L1267-1411.
//!
//! Three extern fns (i64 / f64 / bool → Str) plus the
//! `__torajs_print_f64_js` console.log path that shares the
//! shortest-roundtrip formatter. All preserve the pre-port byte-
//! equal output by routing through libc `snprintf` / `strtod` —
//! Rust's Ryu-based `f64::to_string()` follows the same shortest-
//! roundtrip rule but isn't guaranteed bit-equal to printf's `%g`
//! for every IEEE-754 input, so we keep the libc path for the port
//! (a Ryu-only rewrite is a later perf wedge).
//!
//! ## Spec §6.1.6.1.13 / §22.1.3.6 — f64 ToString
//!
//! - Integer-valued double in `(-1e21, 1e21)` → plain decimal
//!   (`%.0f`), never exponential.
//! - Otherwise → shortest decimal that round-trips via `%.*g` from
//!   precision 1 to 17.
//! - NaN → `"NaN"`, Infinity → `"Infinity"`, -Infinity → `"-Infinity"`.
//! - `-0` to `"0"` for `String(-0)` — but `console.log(-0)` keeps the
//!   sign (so `print_f64_js` does NOT strip the leading `-`; only
//!   `__torajs_f64_to_str` does).

use crate::str_bridge::alloc_str;

unsafe extern "C" {
    // v0.7-A3 Step 14-b: per-byte stdout writer through torajs-io's
    // 0-libc buffered writer. Shared process-global line buffer with
    // __torajs_str_print / __torajs_substr_print / arr_print /
    // inspect / IR-emitted print_i64 / print_bool / print_f64.
    fn __torajs_io_putc_out(c: i32) -> i32;
    // v0.7-A4 Step 15-d: 0-libc shortest-roundtrip f64 → decimal
    // string. Replaces the libc `snprintf` %.*g try-precisions loop
    // + `strtod` round-trip verifier with a single call (Rust
    // core::fmt's Grisu3 produces shortest-roundtrip in one pass,
    // post-processed by torajs-fmt to JS spec §6.1.6.1.13 shape).
    fn __torajs_fmt_dtoa(d: f64, out_buf: *mut u8, out_cap: usize) -> i32;
}

/// f64 → shortest decimal byte representation per JS spec. Writes
/// into `buf` (≥ 32 bytes) and returns the number of bytes written.
/// On overflow / invalid input returns -1.
///
/// v0.7-A4 Step 15-d: delegated to `__torajs_fmt_dtoa` in
/// torajs-fmt (0-libc; core::fmt's Grisu3 + JS-spec post-process).
/// Replaces the prior libc `snprintf` %.*g try-precisions loop
/// + `strtod` round-trip verifier with a single shortest-by-
/// construction call.
pub fn f64_shortest(d: f64, buf: &mut [u8]) -> i32 {
    unsafe { __torajs_fmt_dtoa(d, buf.as_mut_ptr(), buf.len()) }
}

// ============================================================
// extern "C" wrappers
// ============================================================

/// `String(n)` for i64 — fresh Str of the decimal representation.
///
/// v0.7-A4 Step 15-d dropped libc `snprintf("%lld", n)` for
/// `core::fmt::Write` into a stack buffer; rotation 466 dropped
/// `core::fmt` too. The digits were never the cost — `Formatter`
/// and `pad_integral` around them were. `torajs_fmt::itoa` writes
/// the same bytes with neither.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_i64_to_str(n: i64) -> *mut u8 {
    use torajs_fmt::itoa::{ITOA_BUF_LEN, itoa_into};
    let mut buf = [0u8; ITOA_BUF_LEN];
    let start = itoa_into(n, &mut buf);
    alloc_str(&buf[start..])
}

/// JS-spec `String(d)` digits into a caller buffer — the single
/// source for the NaN / ±Infinity literals and the §22.1.3.6
/// `String(-0)` → `"0"` sign strip. `__torajs_f64_to_str` below and
/// torajs-str's fused `__torajs_str_concat_f64` (S1-A2 attack B1)
/// both format through here so the special cases cannot drift
/// apart. (`console.log(-0)` keeps its sign — that path runs
/// through `__torajs_print_f64_js`, which does NOT strip.)
///
/// # Safety
/// `out` must be a writable buffer of at least 32 bytes. Returns
/// the byte count written.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_f64_js_digits(d: f64, out: *mut u8) -> i64 {
    let write = |bytes: &[u8]| -> i64 {
        let dst = unsafe { core::slice::from_raw_parts_mut(out, bytes.len()) };
        dst.copy_from_slice(bytes);
        bytes.len() as i64
    };
    if d.is_nan() {
        return write(b"NaN");
    }
    if d == f64::INFINITY {
        return write(b"Infinity");
    }
    if d == f64::NEG_INFINITY {
        return write(b"-Infinity");
    }
    let mut buf = [0u8; 32];
    let written = f64_shortest(d, &mut buf);
    let mut len = if written < 0 { 0 } else { written as usize };
    let mut off = 0;
    if d == 0.0 && len >= 1 && buf[0] == b'-' {
        off = 1;
        len -= 1;
    }
    write(&buf[off..off + len])
}

/// `String(d)` for f64. NaN / ±Infinity → spec strings. `-0` →
/// `"0"` (sign stripped). Digits via [`__torajs_f64_js_digits`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_f64_to_str(d: f64) -> *mut u8 {
    let mut buf = [0u8; 32];
    let len = unsafe { __torajs_f64_js_digits(d, buf.as_mut_ptr()) };
    alloc_str(&buf[..len as usize])
}

/// `String(b)` for booleans. 1 → "true", 0 → "false".
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_bool_to_str(b: i32) -> *mut u8 {
    if b != 0 {
        alloc_str(b"true")
    } else {
        alloc_str(b"false")
    }
}

/// `console.log(d)` for f64 — writes JS-spec shortest-roundtrip
/// representation + newline directly to stdout via
/// `__torajs_io_putc_out` (shared 0-libc buffered writer with
/// `print_i64` / `print_bool` / `str_print` and IR-emitted print
/// family). NaN / ±Infinity special-cased.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_print_f64_js(d: f64) {
    if d.is_nan() {
        for &b in b"NaN\n" {
            unsafe { __torajs_io_putc_out(b as i32) };
        }
        return;
    }
    if d == f64::INFINITY {
        for &b in b"Infinity\n" {
            unsafe { __torajs_io_putc_out(b as i32) };
        }
        return;
    }
    if d == f64::NEG_INFINITY {
        for &b in b"-Infinity\n" {
            unsafe { __torajs_io_putc_out(b as i32) };
        }
        return;
    }
    let mut buf = [0u8; 32];
    let n = f64_shortest(d, &mut buf);
    let n = if n < 0 { 0 } else { n as usize };
    if n > 0 {
        for &b in &buf[..n] {
            unsafe { __torajs_io_putc_out(b as i32) };
        }
    }
    unsafe { __torajs_io_putc_out(b'\n' as i32) };
}
