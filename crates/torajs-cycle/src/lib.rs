//! Bacon & Rajan trial-deletion cycle collector for the torajs AOT
//! TypeScript runtime.
//!
//! Layer-3 substrate of the architecture rewrite (P4.4, 2026-05-24).
//! Replaces `runtime_cycle.c` — the entire cycle-collector body now
//! lives in this pure-Rust crate.
//!
//! ## Algorithm
//!
//! Three-color trial deletion from D. F. Bacon & V. T. Rajan,
//! "Concurrent Cycle Collection in Reference Counted Systems"
//! (ECOOP 2001). Same approach Python's `gc` + CPython's cyclic-
//! garbage finder use.
//!
//! - **PURPLE** — buffered as a potential cycle root (rc went down
//!   on a cyclic-shape type but stayed positive).
//! - **GRAY** — being marked during a current trial-deletion pass.
//! - **WHITE** — confirmed garbage; freed by the collect phase.
//! - **BLACK** — in use, no cycle suspicion (default state).
//!
//! ## Module split
//!
//! - [`layout`] — heap header + class-layout struct + color helpers +
//!   classification predicates (`is_class_obj`, `is_visitable_arr`,
//!   `has_walkable_children`).
//! - [`arr`] — Array<T> byte-offset accessors (`arr_len_of`,
//!   `arr_slot_at`, `arr_slot_clear`). Standalone — no `torajs-arr`
//!   dep, mirrors the C source's pattern of independent layout
//!   knowledge in the collector.
//! - [`buffer`] — global candidate buffer + `cycle_buffer` /
//!   `cycle_unbuffer` extern fns. AtomicPtr-static pattern (same as
//!   `torajs-weak::registry` + `torajs-arr::pool`).
//! - [`collect`] — the three phases (`mark_gray` / `scan` /
//!   `scan_black` / `collect_white`) + the public `cycle_collect` /
//!   `cycle_at_exit_drain` entry points.
//!
//! ## Cross-tier hooks
//!
//! - `__torajs_value_drop_heap` (from runtime_str.c at `tr build`
//!   link) — invoked by `collect_white` to drop surviving non-cycle
//!   children with their type-specific dec paths.
//! - `__torajs_class_layouts` / `__torajs_n_class_layouts` (emitted
//!   by `ssa_inkwell::compile_module` at codegen) — the per-class
//!   field-offset metadata that lets the walker descend into
//!   class-instance children. Stub (length 0) at cargo test time.
//!
//! ## Scope (matches the C source)
//!
//! - Class instances (TAG_OBJ with declared class_tag) walk via
//!   `__torajs_class_layouts`.
//! - Arrays (TAG_ARR) walk every slot — slots pointing to class
//!   instances or arrays participate. Array<Any>'s 16B slot stride
//!   isn't decoded yet — only Array<T>'s 8B slots.
//! - Closures leak: their env layout isn't reachable from the
//!   runtime side. Lands as a follow-up.
//! - Manual `gc()` trigger + threshold-driven auto-collect +
//!   main-exit drain. Single-threaded; non-concurrent.

pub mod arr;
pub mod buffer;
pub mod collect;
pub mod layout;

pub use buffer::{__torajs_cycle_buffer, __torajs_cycle_unbuffer};
pub use collect::{__torajs_cycle_at_exit_drain, __torajs_cycle_collect};

// Cross-tier extern stubs for cargo unit tests — `__torajs_value_drop_heap`
// is provided by runtime_str.c at `tr build` link time; cargo test
// doesn't link that, so a panicking stub keeps the test binary
// linking clean.
#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_value_drop_heap(_p: *mut core::ffi::c_void) {
    panic!(
        "torajs-cycle unit-test stub: __torajs_value_drop_heap should not be called from cargo test paths"
    );
}
