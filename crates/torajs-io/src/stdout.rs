//! Stdout extern entry points. Three `#[unsafe(no_mangle)] pub
//! extern "C"` symbols replace every libc `putchar` /
//! buffered-write call site in the torajs runtime + IR-emitted
//! print family:
//!
//! - [`__torajs_io_putc_stdout`] — 1:1 putchar(c) drop-in.
//!   Returns the byte cast to `i32` (matches libc's putchar
//!   return semantics). Never returns -1; errors are swallowed
//!   per stdio convention.
//! - [`__torajs_io_write_stdout`] — bulk write(buf, len), for
//!   callers that are stdout by definition (`process.stdout.write`).
//!   The print family batches through the sink module's `write_out`
//!   twin instead (torajs-print's digit buffers land there).
//! - [`__torajs_io_flush`] — explicit drain. Emitted at process
//!   exit hook before `exit(0)` to ensure all buffered bytes
//!   reach stdout.

use crate::buf::{STDOUT, STDOUT_FD};

/// Push one byte to the stdout line buffer. Drop-in replacement
/// for libc `putchar(c)`. Returns the byte (cast to `i32`).
///
/// # Safety
///
/// Single-threaded torajs user binary scope. No concurrent
/// callers per v0.7. When v0.8 adds threads, `STDOUT` becomes
/// `#[thread_local]`.
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_io_putc_stdout(c: i32) -> i32 {
    // Safety: caller covenant — single-threaded access. Cast c
    // (which may carry sign-extended high bits from C `int`) to
    // u8; matches libc putchar's "unsigned char value" contract.
    unsafe { STDOUT.push(STDOUT_FD, c as u8) };
    c
}

/// Bulk-write `len` bytes from `buf` to stdout. Buffer is
/// process-global line buffer; newlines within the slice
/// trigger sub-flushes.
///
/// # Safety
///
/// `buf` must point at ≥ `len` readable bytes. Caller is
/// responsible for the slice's validity. Single-threaded
/// scope (see [`__torajs_io_putc_stdout`]).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_io_write_stdout(buf: *const u8, len: u64) {
    if buf.is_null() || len == 0 {
        return;
    }
    let s = unsafe { core::slice::from_raw_parts(buf, len as usize) };
    unsafe { STDOUT.write(STDOUT_FD, s) };
}

/// Drain all buffered io bytes — stdout then stderr — each via a
/// single `torajs_syscall::write(fd, &buf[0..len])`. Emit at
/// process exit hook to ensure no trailing-without-newline output
/// is lost on either stream. (Since RFC 20260812-console-sink the
/// stderr stream is line-buffered too; the exit hook's "drain
/// everything" semantics now covers both.)
///
/// # Safety
///
/// Single-threaded scope.
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_io_flush() {
    // SAFETY: see __torajs_io_putc_stdout.
    unsafe { STDOUT.flush(STDOUT_FD) };
    unsafe { crate::buf::STDERR.flush(crate::buf::STDERR_FD) };
}
