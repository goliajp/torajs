//! Line-width estimate (inspect wrap trunk) — mirror of bun 1.3.14's
//! `Formatter.estimated_line_length` (ConsoleObject.zig:1028-1042).
//! The estimate is deliberately approximate the same way bun's is:
//! string quotes and ANSI codes are NOT counted, commas / separator
//! spaces / digits / literal keyword widths ARE. Wrap decisions
//! compare `estimate > 80` (strictly greater) after each comma.
//!
//! Multi-thread-ready shape (design-principles §6.2): a process-wide
//! atomic, not `static mut` / lock. Concurrent console.log already
//! interleaves at the byte level; when stdout gains its per-thread
//! serialization this estimate joins that same lock domain.
//!
//! Extracted from [`super::formatters`] as the rotation-196 file-size
//! sweep (parent had drifted to 522 LOC over the inspect-family
//! chunk history). Verbatim moves.

use core::sync::atomic::{AtomicU32, Ordering};

static INSPECT_LINE_EST: AtomicU32 = AtomicU32::new(0);

/// Reset the line estimate to `cols` (bun's `resetLine()` — called
/// with the pad column just written after a line break).
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_inspect_line_reset(cols: u32) {
    INSPECT_LINE_EST.store(cols, Ordering::Relaxed);
}

/// Add `n` estimated columns to the current line (bun's
/// `addForNewLine(n)`).
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_inspect_line_add(n: u32) {
    INSPECT_LINE_EST.fetch_add(n, Ordering::Relaxed);
}

/// Current line estimate — callers test `> 80` for wrap decisions
/// (bun's `goodTimeForANewLine` without the reset side effect; the
/// caller resets explicitly after emitting the break + pad).
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_inspect_line_len() -> u32 {
    INSPECT_LINE_EST.load(Ordering::Relaxed)
}
