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

#![feature(optimize_attribute)]

use torajs_fmt::itoa::{ITOA_BUF_LEN, itoa_into};

unsafe extern "C" {
    fn __torajs_io_write_out(buf: *const u8, len: u64);
    fn __torajs_print_f64_js(d: f64);
    fn __torajs_print_f64_js_inline(d: f64);
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

/// r505 — `print_bool` without the newline: one argument of a
/// multi-arg `console.log`, whose lowering writes the separators and
/// the line end itself. The `_inline` family (i64 / f64 / bool / str
/// / substr) is what keeps a typed multi-arg log off the any-value
/// printer — that printer roots every inspectable world (98 KB on
/// `console.log(1, 2)`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn print_bool_inline(b: bool) {
    let bytes: &[u8] = if b { b"true" } else { b"false" };
    write_all(bytes);
}

/// `_print_i64(n)` — writes the base-10 decimal representation
/// of `n` followed by `\n`. The digits come from torajs-fmt's
/// [`itoa_into`] — the one i64 formatter every print / join /
/// `String(n)` site shares — so this crate carries no digit loop of
/// its own. The longest rendering is
/// `i64::MIN = -9_223_372_036_854_775_808` (20 chars) + `'\n'`,
/// which fits the on-stack buffer with no allocation.
///
/// The previous body kept a private one-digit-at-a-time loop, and
/// LLVM unrolled its 20 bounded iterations in full — a magic-constant
/// divide per digit — into 1,584 bytes of `__text` that every
/// `console.log(int)` program paid (s3 rotation 504 census). One
/// kernel — and, here, the rolled shape of it: `console.log(int)`
/// is bounded by the write behind it, so this entry is compiled for
/// size and LLVM leaves the two-digit loop as a loop (the inlined
/// copies in the string-building kernels keep their own tuning).
#[unsafe(no_mangle)]
#[optimize(size)]
pub unsafe extern "C" fn print_i64(n: i64) {
    let mut out = [0u8; I64_LINE_LEN];
    let len = render_i64_line(n, &mut out);
    write_all(&out[..len]);
}

/// `print_i64` without the newline (see [`print_bool_inline`]): the
/// same rendered line, minus its last byte.
#[unsafe(no_mangle)]
#[optimize(size)]
pub unsafe extern "C" fn print_i64_inline(n: i64) {
    let mut out = [0u8; I64_LINE_LEN];
    let len = render_i64_line(n, &mut out);
    if let Some(digits) = out.get(..len.saturating_sub(1)) {
        write_all(digits);
    }
}

/// `"-9223372036854775808\n"` — the longest line `print_i64` writes.
const I64_LINE_LEN: usize = ITOA_BUF_LEN + 1;

/// Render `n` as `<decimal>\n` into the head of `out`; returns the
/// byte count. Pure — the one piece of `print_i64` a unit test can
/// hold, since the entry point itself only writes to the sink.
fn render_i64_line(n: i64, out: &mut [u8; I64_LINE_LEN]) -> usize {
    let mut digits = [0u8; ITOA_BUF_LEN];
    let start = itoa_into(n, &mut digits);
    let len = ITOA_BUF_LEN - start;
    // Iterator forms, not range indexing: an indexed copy carries a
    // formatted panic path, and one of those in a runtime kernel
    // drags `core::fmt` into every program that links it.
    for (dst, &b) in out.iter_mut().zip(digits.iter().skip(start)) {
        *dst = b;
    }
    if let Some(nl) = out.get_mut(len) {
        *nl = b'\n';
    }
    len + 1
}

/// `_print_f64(d)` — delegates to `__torajs_print_f64_js` in
/// `libtorajs_num.a`. Keeps ES spec NaN / Infinity / signed-zero
/// formatting in a single source-of-truth implementation rather
/// than duplicating Ryū / dtoa logic.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn print_f64(d: f64) {
    unsafe { __torajs_print_f64_js(d) }
}

/// `print_f64` without the newline (see [`print_bool_inline`]).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn print_f64_inline(d: f64) {
    unsafe { __torajs_print_f64_js_inline(d) }
}

#[cfg(test)]
mod tests {
    // The entry points write to the current sink, so the tests drive
    // the pure renderer they share and pin the bytes it produces.
    use super::*;

    fn line(n: i64) -> String {
        let mut out = [0u8; I64_LINE_LEN];
        let len = render_i64_line(n, &mut out);
        std::str::from_utf8(&out[..len]).unwrap().to_string()
    }

    /// `i64::MIN` has no positive counterpart — the magnitude must be
    /// taken without negating. Also the longest line: 21 bytes fills
    /// the buffer exactly.
    #[test]
    fn i64_min_does_not_overflow() {
        assert_eq!(line(i64::MIN), "-9223372036854775808\n");
        assert_eq!(line(i64::MIN).len(), 21);
    }

    #[test]
    fn i64_zero_emits_zero_newline() {
        assert_eq!(line(0), "0\n");
    }

    #[test]
    fn i64_positive_and_negative_round_trip() {
        assert_eq!(line(4_277_891), "4277891\n");
        assert_eq!(line(-7), "-7\n");
        assert_eq!(line(i64::MAX), "9223372036854775807\n");
    }
}
