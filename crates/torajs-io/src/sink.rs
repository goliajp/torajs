//! Current-sink indirection for the print/inspect family — the
//! RFC 20260812-console-sink primitive layer.
//!
//! The runtime print walkers (torajs-str / -num / -bigint / -arr /
//! -collections / -promise / -meta / -dynobj / -anyvalue inspect /
//! -fnname / -regex) stream bytes through these `_out` entry
//! points instead of the fd-literal `putc_stdout` family. The
//! current sink defaults to STDOUT (byte-for-byte the legacy
//! behavior); `console.error` / `console.warn` wrappers bracket a
//! print call with [`__torajs_io_sink_to_stderr`] /
//! [`__torajs_io_sink_to_stdout`].
//!
//! ## Interleaving contract
//!
//! Every sink switch drains the buffer it is leaving before the
//! flip, so under `2>&1` redirection the byte order on the shared
//! fd matches caller order — the generalization of the legacy
//! convention (`__torajs_str_print_err` flushed stdout before its
//! raw `write(2)`).
//!
//! ## Threading
//!
//! The switch is an `AtomicBool` (Relaxed — no cross-thread
//! ordering to protect in the v0.7 single-threaded covenant, and
//! the multi-thread-ready shape per design-principles §6.2). When
//! v0.8 threads land, the LineBufs and this flag move to
//! `#[thread_local]` together — a per-thread current sink is the
//! natural semantics (each thread's console redirection is its
//! own).

use crate::buf::{LineBuf, STDERR, STDOUT};
use core::sync::atomic::{AtomicBool, Ordering};

/// Current-sink flag: `false` = STDOUT (default), `true` = STDERR.
static CUR_IS_STDERR: AtomicBool = AtomicBool::new(false);

#[inline]
fn cur() -> &'static LineBuf {
    if CUR_IS_STDERR.load(Ordering::Relaxed) {
        &STDERR
    } else {
        &STDOUT
    }
}

/// Push one byte to the current sink. Drop-in replacement for
/// [`crate::stdout::__torajs_io_putc_stdout`] at every
/// print-family call site; identical behavior while the sink is
/// STDOUT (the default).
///
/// # Safety
///
/// Single-threaded torajs user binary scope (see
/// [`crate::stdout::__torajs_io_putc_stdout`]).
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_io_putc_out(c: i32) -> i32 {
    // SAFETY: caller covenant — single-threaded access.
    unsafe { cur().push(c as u8) };
    c
}

/// Bulk-write `len` bytes from `buf` to the current sink.
///
/// # Safety
///
/// `buf` must point at ≥ `len` readable bytes; single-threaded
/// scope (see [`crate::stdout::__torajs_io_write_stdout`]).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_io_write_out(buf: *const u8, len: u64) {
    if buf.is_null() || len == 0 {
        return;
    }
    let s = unsafe { core::slice::from_raw_parts(buf, len as usize) };
    // SAFETY: caller covenant — single-threaded access.
    unsafe { cur().write(s) };
}

/// Route subsequent `_out` writes to STDERR. Drains STDOUT first
/// (interleaving contract). Emitted by the `console.error` /
/// `console.warn` lowering before the print-target call.
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_io_sink_to_stderr() {
    // SAFETY: single-threaded covenant (see module doc).
    unsafe { STDOUT.flush() };
    CUR_IS_STDERR.store(true, Ordering::Relaxed);
}

/// Route subsequent `_out` writes back to STDOUT. Drains STDERR
/// first (interleaving contract). Emitted after the print-target
/// call to restore the default sink.
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_io_sink_to_stdout() {
    // SAFETY: single-threaded covenant (see module doc).
    unsafe { STDERR.flush() };
    CUR_IS_STDERR.store(false, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The default sink is STDOUT and the switch round-trips.
    /// (No write() calls — that would clobber the test runner's
    /// streams; we only exercise the flag.)
    #[test]
    fn sink_flag_round_trip() {
        assert!(!CUR_IS_STDERR.load(Ordering::Relaxed));
        __torajs_io_sink_to_stderr();
        assert!(CUR_IS_STDERR.load(Ordering::Relaxed));
        __torajs_io_sink_to_stdout();
        assert!(!CUR_IS_STDERR.load(Ordering::Relaxed));
    }
}
