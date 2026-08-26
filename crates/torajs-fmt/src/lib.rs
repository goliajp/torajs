//! torajs-fmt — 0-libc f64 ↔ string conversion.
//!
//! Replaces every libc `snprintf` (f64 → string) and `strtod`
//! (string → f64) call site in tr-built user binaries:
//!
//! - [`dtoa`] — Ryū algorithm (Adams, PLDI 2018). Shortest-
//!   roundtrip f64 → decimal string in one pass.
//! - [`atod`] — Eisel-Lemire algorithm (Lemire, SP&E 2021).
//!   State-of-the-art string → f64 parser; correct on 100% of
//!   inputs (slow-path bigint fallback for the ~0.0001% of
//!   inputs that don't fast-path).
//!
//! ## Layering
//!
//! - `dtoa` module: pure-core implementation. Public extern
//!   `__torajs_fmt_dtoa(d, out_buf, out_cap)` writes the
//!   shortest-roundtrip decimal representation. Caller-owned
//!   buffer ≥ 32 bytes.
//! - `atod` module: pure-core implementation. Public extern
//!   `__torajs_fmt_atod(s, len, endp)` parses the leading
//!   numeric span; advances `endp` past the consumed bytes.
//!
//! ## Why
//!
//! v0.7-A4 phase per `docs/v0.7-A4-finding.md`. After 15-d/e
//! ship, `nm /tmp/<bench-bin> | grep -cE "(snprintf|strtod)"`
//! == 0 for every bench fixture. Combined with A3 (io cutover)
//! and pending A5 (libc-shim audit), user binaries lose their
//! libc dependency entirely (otool -L: 0 libSystem entries).

// Not yet `#![no_std]` — Step 15-a scaffold leaves std on so
// the test binary has a panic_handler. Step 15-b/c introduces
// no_std once the algorithm bodies land + the panic-handler
// wiring is settled (likely a torajs-abort path dep providing
// the handler symbol via its own staticlib, mirroring how
// torajs-str / torajs-arr / etc. do it).

#![feature(optimize_attribute)]

pub mod atod;
pub mod dtoa;
pub mod itoa;
pub mod ryu;
mod ryu_tables;

pub use atod::__torajs_fmt_atod;
pub use dtoa::__torajs_fmt_dtoa;
pub use itoa::__torajs_fmt_itoa;
