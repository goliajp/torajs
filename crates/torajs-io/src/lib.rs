//! torajs-io — 0-libc buffered stdout/stderr writer.
//!
//! Replaces every `extern "C" putchar` call site in the print
//! family (torajs-str / torajs-arr / torajs-anyvalue / torajs-num
//! + ssa_inkwell IR-emitted `define_print_*`). Buffers per-byte
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
//! - `stderr` module: stubbed for v0.7 (A5 final libc verify
//!   handles the stderr path; A3 scope is stdout-only).
//!
//! ## Why
//!
//! v0.7-A3 phase per `docs/v0.7-A3-finding.md`. After 14-c ship,
//! `otool -L /tmp/<bench-bin> | grep -c putchar` == 0 for every
//! print-emitting fixture. The remaining libc surface in user
//! binaries reduces to the alloc family (handled by A5).

#![no_std]

pub mod buf;
pub mod stdout;

pub use stdout::{__torajs_io_flush, __torajs_io_putc_stdout, __torajs_io_write_stdout};
