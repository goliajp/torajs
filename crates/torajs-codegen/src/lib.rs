//! torajs-codegen — in-house aarch64 backend.
//!
//! See `.claude/rfcs/20260604-p6-torajs-codegen/design.md` for the
//! design decisions, sub-step plan (S1..S6), and cut-over protocol.
//!
//! S1 surface (this commit): scaffold + `fn one_plus_two() -> i64 {1+2}`
//! shape compiles end-to-end through `compile_function`, emitted byte
//! stream matches hand-encoded reference (see `compile::tests`).
//!
//! Pipeline shape (matches RFC D1: single-pass-over-SSA, no mid-IR):
//!
//! ```text
//!   torajs_core::ssa::Function
//!       │
//!       ▼  regalloc::allocate     (Linear Scan — trivial in S1, full in S2+)
//!   Assignment { value -> Gpr }
//!       │
//!       ▼  compile::compile_function
//!   CompiledFunction { bytes, relocs, frame_size }
//!       │
//!       ▼  (handed off to torajs-obj #7)
//!   Mach-O / ELF container
//! ```

#![forbid(unsafe_code)]

pub mod compile;
pub mod enc;
pub mod frame;
pub mod linear_scan;
mod linear_scan_lanes;
pub(crate) mod linear_scan_sweep;
pub mod liveness;
pub mod reg;
pub mod regalloc;
pub mod reloc;
pub mod spill_weight;

pub use compile::{
    CompiledFunction, compile_function, compile_function_with, compile_function_with_sigs,
};
pub use reloc::{Reloc, RelocKind};
