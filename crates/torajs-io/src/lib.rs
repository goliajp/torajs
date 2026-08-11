//! torajs-io — 0-libc buffered stdout/stderr writer.
//!
//! Replaces every `extern "C" putchar` call site in the print
//! family (torajs-str / torajs-arr / torajs-anyvalue / torajs-num
//! + torajs-print's `print_*` helpers). Buffers per-byte
//! pushes in a thread-local line buffer; flushes on newline,
//! buffer full, or explicit `__torajs_io_flush()` call.
//!
//! ## Layering
//!
//! - `buf` module: pure `[u8; N]` ring per fd. No allocator
//!   dependency. Mutex-free (single-threaded torajs user binary
//!   per v0.7 scope; cross-thread io is a v0.8+ concern).
//! - `stdout` module: 3 `#[unsafe(no_mangle)] pub extern "C"`
//!   entry points (`putc_stdout` / `write_stdout` / `flush`).
//!   Same calling convention as libc's `putchar` (1:1 drop-in).
//! - `sink` module: current-sink indirection (`putc_out` /
//!   `write_out` + the stderr/stdout switch pair) — the
//!   RFC 20260812-console-sink layer the print/inspect family
//!   streams through so `console.error` / `console.warn` can
//!   redirect a whole walk to STDERR without per-type `_err`
//!   symbol duplication.
//!
//! ## Why
//!
//! v0.7-A3 phase per `docs/v0.7-A3-finding.md`. After 14-c ship,
//! `otool -L /tmp/<bench-bin> | grep -c putchar` == 0 for every
//! print-emitting fixture. The remaining libc surface in user
//! binaries reduces to the alloc family (handled by A5).

#![no_std]

pub mod buf;
pub mod sink;
pub mod stdout;

pub use sink::{
    __torajs_io_putc_out, __torajs_io_sink_to_stderr, __torajs_io_sink_to_stdout,
    __torajs_io_write_out,
};
pub use stdout::{__torajs_io_flush, __torajs_io_putc_stdout, __torajs_io_write_stdout};
