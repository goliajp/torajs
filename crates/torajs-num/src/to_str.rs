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

use core::ffi::c_char;

use crate::str_bridge::alloc_str;

unsafe extern "C" {
    fn snprintf(buf: *mut c_char, n: usize, fmt: *const u8, ...) -> i32;
    fn strtod(s: *const c_char, endp: *mut *mut c_char) -> f64;
    // Per-byte stdout writer — shared C stdio buffer with the rest
    // of the print family (print_i64 / print_bool / str_print).
    // Direct fwrite would diverge buffering (see
    // torajs-str::print module docs).
    fn putchar(c: i32) -> i32;
}

/// f64 → shortest decimal byte representation per JS spec. Writes
/// into `buf` (≥ 32 bytes) and returns the number of bytes written
/// (excluding any NUL). On overflow returns -1.
///
/// Integer-valued doubles in `(-1e21, 1e21)` go through `%.0f` (no
/// exponential notation, per §6.1.6.1.13 step 5). Otherwise try
/// precisions 1..=17 of `%.*g` until one round-trips via `strtod`.
/// Slow vs Ryu/Grisu (up to 17 snprintf calls) but only the print
/// path hits it; output is byte-equal to v8/JSC for every f64.
pub fn f64_shortest(d: f64, buf: &mut [u8]) -> i32 {
    let cap = buf.len();
    // Integer-valued + in spec's plain-decimal range
    let abs_d = if d < 0.0 { -d } else { d };
    if d == d.floor() && abs_d < 1e21 {
        return unsafe { snprintf(buf.as_mut_ptr() as *mut c_char, cap, b"%.0f\0".as_ptr(), d) };
    }
    // Otherwise: try-precisions loop. Stop at the first %.*g that
    // round-trips back to the same f64 via strtod.
    for prec in 1i32..=17 {
        let written = unsafe {
            snprintf(
                buf.as_mut_ptr() as *mut c_char,
                cap,
                b"%.*g\0".as_ptr(),
                prec,
                d,
            )
        };
        if written < 0 || written as usize >= cap {
            return -1;
        }
        let parsed = unsafe { strtod(buf.as_ptr() as *const c_char, core::ptr::null_mut()) };
        if parsed == d {
            return written;
        }
    }
    // Fall-through: 17 didn't round-trip — should not happen for any
    // finite f64. Re-emit at %.17g to match the C runtime's last-
    // resort path.
    unsafe { snprintf(buf.as_mut_ptr() as *mut c_char, cap, b"%.17g\0".as_ptr(), d) }
}

// ============================================================
// extern "C" wrappers
// ============================================================

/// `String(n)` for i64 — fresh Str of the decimal representation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_i64_to_str(n: i64) -> *mut u8 {
    let mut buf = [0u8; 24];
    let written = unsafe {
        snprintf(
            buf.as_mut_ptr() as *mut c_char,
            buf.len(),
            b"%lld\0".as_ptr(),
            n as core::ffi::c_longlong,
        )
    };
    let len = if written < 0 { 0 } else { written as usize };
    alloc_str(&buf[..len])
}

/// `String(d)` for f64. NaN / ±Infinity → spec strings. `-0` →
/// `"0"` (sign stripped). All other values use [`f64_shortest`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_f64_to_str(d: f64) -> *mut u8 {
    if d.is_nan() {
        return alloc_str(b"NaN");
    }
    if d == f64::INFINITY {
        return alloc_str(b"Infinity");
    }
    if d == f64::NEG_INFINITY {
        return alloc_str(b"-Infinity");
    }
    let mut buf = [0u8; 32];
    let written = f64_shortest(d, &mut buf);
    let mut len = if written < 0 { 0 } else { written as usize };
    let mut off = 0;
    // §22.1.3.6: String(-0) → "0" (no sign). console.log(-0) keeps
    // the sign — that path runs through __torajs_print_f64_js below
    // which does NOT strip.
    if d == 0.0 && len >= 1 && buf[0] == b'-' {
        off = 1;
        len -= 1;
    }
    alloc_str(&buf[off..off + len])
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
/// representation + newline directly to stdout via libc `fwrite` /
/// `putchar` (shared buffer with `print_i64` / `print_bool` /
/// `str_print`). NaN / ±Infinity special-cased.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_print_f64_js(d: f64) {
    if d.is_nan() {
        for &b in b"NaN\n" {
            unsafe { putchar(b as i32) };
        }
        return;
    }
    if d == f64::INFINITY {
        for &b in b"Infinity\n" {
            unsafe { putchar(b as i32) };
        }
        return;
    }
    if d == f64::NEG_INFINITY {
        for &b in b"-Infinity\n" {
            unsafe { putchar(b as i32) };
        }
        return;
    }
    let mut buf = [0u8; 32];
    let n = f64_shortest(d, &mut buf);
    let n = if n < 0 { 0 } else { n as usize };
    if n > 0 {
        for &b in &buf[..n] {
            unsafe { putchar(b as i32) };
        }
    }
    unsafe { putchar(b'\n' as i32) };
}
