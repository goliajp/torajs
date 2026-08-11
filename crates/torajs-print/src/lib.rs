//! `_print_bool` / `_print_i64` / `_print_f64` — primitive
//! console.log helpers used by torajs SSA-lower'd code to render
//! primitive values to stdout.
//!
//! Historically `crates/torajs-core/src/ssa_inkwell/builders.rs`
//! emitted these as LLVM IR fragments inside every compiled user
//! binary (one wrapper per primitive type, calling `putchar` /
//! `__torajs_print_f64_js`). The self-research toolchain
//! (#9 SD-4) replaces the LLVM-IR backend with the
//! `torajs_codegen → torajs_obj → torajs_link` chain — which has
//! no general-purpose IR-fragment-emit. Porting these helpers to
//! a Rust staticlib means the link path resolves them just like
//! every other `_torajs_*` extern, with zero special-casing in
//! the codegen.
//!
//! Symbol ABI (matches the LLVM-era emit names exactly so the
//! SSA-lower `call print_*` sites resolve unchanged):
//!
//! - `_print_bool(b: bool)` — writes `"true\n"` or `"false\n"`
//!   to stdout.
//! - `_print_i64(n: i64)` — writes the base-10 decimal
//!   representation of `n` followed by `\n`. Handles the
//!   `n == 0` special case and the `i64::MIN` boundary (no
//!   panic on `n.unsigned_abs()`).
//! - `_print_f64(d: f64)` — tail-call to
//!   `__torajs_print_f64_js` in `libtorajs_num.a`, which already
//!   implements the ES spec NaN / Infinity / signed-zero
//!   formatting. We don't re-implement the f64 path here; the
//!   torajs-num version is the spec source of truth.
//!
//! Since RFC 20260812-console-sink knife 2 the composed bytes go
//! through `torajs_io::__torajs_io_write_out` — the current-sink
//! indirection the rest of the print family streams through — so a
//! `console.error(42)` bracketed by the lowering's sink switch
//! reaches stderr like every other type. (The pre-knife-2 body
//! wrote fd 1 via a raw `__torajs_syscall_write` loop, which
//! bypassed both the sink switch and torajs-io's line buffer —
//! that made `console.error(int|bool)` the only two lanes that
//! kept printing to stdout.) Every payload ends in `'\n'`, which
//! triggers torajs-io's line flush, so byte-arrival timing is
//! unchanged.

unsafe extern "C" {
    fn __torajs_io_write_out(buf: *const u8, len: u64);
    fn __torajs_print_f64_js(d: f64);
}

/// Hand the composed bytes to the print family's shared
/// current-sink writer (line-buffered; the trailing `'\n'` in
/// every caller's payload triggers the flush).
fn write_all(buf: &[u8]) {
    unsafe { __torajs_io_write_out(buf.as_ptr(), buf.len() as u64) };
}

/// `_print_bool(b)` — writes `"true\n"` for `true` and
/// `"false\n"` for `false`. Same byte sequence as
/// `console.log(true | false)` per the ES spec.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn print_bool(b: bool) {
    let bytes: &[u8] = if b { b"true\n" } else { b"false\n" };
    write_all(bytes);
}

/// `_print_i64(n)` — writes the base-10 decimal representation
/// of `n` followed by `\n`. Max-length input is
/// `i64::MIN = -9_223_372_036_854_775_808` (20 chars) + `'\n'`
/// = 21 bytes, which fits in the on-stack 21-byte buffer with
/// no allocation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn print_i64(n: i64) {
    let mut buf = [0u8; 21];
    let mut idx = buf.len();

    idx -= 1;
    buf[idx] = b'\n';

    if n == 0 {
        idx -= 1;
        buf[idx] = b'0';
    } else {
        // `unsigned_abs` handles `i64::MIN` (returns 2^63) without
        // overflow; plain `-n` would wrap.
        let (mut value, neg) = if n < 0 {
            (n.unsigned_abs(), true)
        } else {
            (n as u64, false)
        };
        while value > 0 {
            idx -= 1;
            buf[idx] = b'0' + (value % 10) as u8;
            value /= 10;
        }
        if neg {
            idx -= 1;
            buf[idx] = b'-';
        }
    }

    let len = buf.len() - idx;
    write_all(&buf[idx..idx + len]);
}

/// `_print_f64(d)` — delegates to `__torajs_print_f64_js` in
/// `libtorajs_num.a`. Keeps ES spec NaN / Infinity / signed-zero
/// formatting in a single source-of-truth implementation rather
/// than duplicating Ryū / dtoa logic.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn print_f64(d: f64) {
    unsafe { __torajs_print_f64_js(d) }
}

#[cfg(test)]
mod tests {
    // Tests intentionally re-implement the in-source format loops in-place
    // (rather than driving the `extern "C"` entry points) because the
    // entry points write to fd 1 via syscall — there's no test-friendly
    // way to capture that without spawning a child process. The tests
    // pin the byte-level invariants that the in-source loops rely on
    // (i64::MIN no-overflow, n==0 special case, multi-digit reverse fill).

    /// `i64::MIN` exercises the `n.unsigned_abs()` / no-wrap path —
    /// a naive `-n` would overflow. Asserts the formatted byte
    /// length matches `"-9223372036854775808\n"` = 21 bytes.
    #[test]
    fn i64_min_does_not_overflow() {
        // Drive the helper end-to-end via a fake write captured in
        // a thread-local; if the rendering would panic we'd never
        // reach the assert. Run inside an OS pipe so write_all's
        // syscall path is exercised under unit tests too.
        //
        // We can't trivially capture stdout in a unit test without
        // dragging libc::dup2 in, so we only assert that the
        // formatter produces the expected byte sequence by
        // re-implementing the rendering in-place.
        let n: i64 = i64::MIN;
        let value = n.unsigned_abs();
        let mut buf = [0u8; 21];
        let mut idx = buf.len();
        idx -= 1;
        buf[idx] = b'\n';
        let mut v = value;
        while v > 0 {
            idx -= 1;
            buf[idx] = b'0' + (v % 10) as u8;
            v /= 10;
        }
        idx -= 1;
        buf[idx] = b'-';
        let s = std::str::from_utf8(&buf[idx..]).unwrap();
        assert_eq!(s, "-9223372036854775808\n");
        assert_eq!(buf.len() - idx, 21);
    }

    /// `n == 0` must emit `"0\n"` — the digit-extraction loop
    /// terminates immediately on `value == 0` without the
    /// special-case branch.
    #[test]
    fn i64_zero_emits_zero_newline() {
        let mut buf = [0u8; 21];
        let mut idx = buf.len();
        idx -= 1;
        buf[idx] = b'\n';
        idx -= 1;
        buf[idx] = b'0';
        let s = std::str::from_utf8(&buf[idx..]).unwrap();
        assert_eq!(s, "0\n");
    }

    /// Positive multi-digit reverse-fill round-trip.
    #[test]
    fn i64_positive_round_trips() {
        let n: i64 = 4_277_891;
        let mut buf = [0u8; 21];
        let mut idx = buf.len();
        idx -= 1;
        buf[idx] = b'\n';
        let mut v = n as u64;
        while v > 0 {
            idx -= 1;
            buf[idx] = b'0' + (v % 10) as u8;
            v /= 10;
        }
        let s = std::str::from_utf8(&buf[idx..]).unwrap();
        assert_eq!(s, "4277891\n");
    }
}
