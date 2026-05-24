//! `console.error` / `console.warn` primitive print fns for non-Str
//! types — port of `runtime_str.c` L1730-1738.
//!
//! Three sibling fns matching `console.log`'s three-way SSA
//! dispatch (i64 / f64 / bool), but routed to stderr (fd 2) instead
//! of stdout. The Str-typed `__torajs_str_print_err` lives in
//! `torajs-str::print` and was ported in P3.1-g.1.
//!
//! ## Buffering / output channel
//!
//! Uses POSIX `dprintf` (formatted writes direct to fd 2, no FILE *
//! buffer) and libc `write` (raw bytes). Both share fd 2 with the
//! still-C-side `__torajs_panic` / backtrace / `process.stderr.write`
//! callers, so ordering is preserved at the kernel level (each
//! single syscall is atomic; inter-call ordering matches caller
//! order). Avoids the `stderr` symbol naming asymmetry (`stderr` on
//! glibc, `__stderrp` on macOS) — `dprintf` takes an fd directly.
//!
//! ## Format
//!
//! Matches the pre-port C runtime bit-for-bit:
//!
//! | fn                  | format spec   | example       |
//! |---------------------|---------------|---------------|
//! | `print_i64_err`     | `%lld\n`      | `42\n`        |
//! | `print_f64_err`     | `%g\n`        | `1.5\n`       |
//! | `print_bool_err`    | `true\n` / `false\n` | n/a    |
//!
//! `%g` is C's shortest-of-%e-or-%f with 6 sig digits. This
//! diverges from JS `Number.prototype.toString` semantics (which
//! uses 17-digit shortest-roundtrip), but matches the pre-port
//! conformance state — a JS-spec-correct stderr-side formatter
//! belongs to a later wedge after the rewrite stabilizes (the
//! stdout-side `__torajs_print_f64_js` already does spec-correct
//! formatting; once it ports to Rust it can be reused here).

use core::ffi::c_void;

const STDERR_FILENO: i32 = 2;

unsafe extern "C" {
    fn dprintf(fd: i32, fmt: *const u8, ...) -> i32;
    fn write(fd: i32, buf: *const c_void, count: usize) -> isize;
}

#[inline]
unsafe fn write_all_fd2(buf: &[u8]) {
    // Loop over short writes — `write(2)` may return fewer bytes
    // than requested under signal interruption. fd 2 is unbuffered
    // so EINTR is the realistic short-write source.
    let mut p = buf.as_ptr() as *const c_void;
    let mut n = buf.len();
    while n > 0 {
        let written = unsafe { write(STDERR_FILENO, p, n) };
        if written <= 0 {
            return; // EBADF / EPIPE — drop silently, matches libc's
            // fputs behavior on a closed stderr.
        }
        let w = written as usize;
        p = unsafe { (p as *const u8).add(w) as *const c_void };
        n -= w;
    }
}

/// `console.error(int64)` — stderr `%lld\n`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_print_i64_err(n: i64) {
    let _ = unsafe {
        dprintf(
            STDERR_FILENO,
            b"%lld\n\0".as_ptr(),
            n as core::ffi::c_longlong,
        )
    };
}

/// `console.error(f64)` — stderr `%g\n`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_print_f64_err(d: f64) {
    let _ = unsafe { dprintf(STDERR_FILENO, b"%g\n\0".as_ptr(), d) };
}

/// `console.error(bool)` — stderr `true\n` / `false\n`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_print_bool_err(b: i64) {
    let bytes: &[u8] = if b != 0 { b"true\n" } else { b"false\n" };
    unsafe { write_all_fd2(bytes) };
}
