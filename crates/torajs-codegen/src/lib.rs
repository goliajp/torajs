//! torajs-codegen — in-house aarch64 backend.
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

mod coalesce;
pub mod compile;
pub mod enc;
pub mod frame;
pub mod linear_scan;
mod linear_scan_alloc;
mod linear_scan_calls;
mod linear_scan_dump;
mod linear_scan_lanes;
mod linear_scan_rehome;
mod linear_scan_spill;
pub(crate) mod linear_scan_sweep;
mod live_ranges;
pub mod liveness;
pub mod liveness_blocks;
pub mod reg;
pub mod regalloc;
pub mod reloc;
mod shrink_wrap;
pub mod spill_weight;
mod switches;
mod vec_homes;

pub use compile::{
    CellOps, CompiledFunction, InlineSites, ModuleFacts, StrAppendLit, compile_function,
    compile_function_in, compile_function_with,
};
pub use reloc::{Reloc, RelocKind};
pub use torajs_core::ssa::{id_hash, is_local_label};
