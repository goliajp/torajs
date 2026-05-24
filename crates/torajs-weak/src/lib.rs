//! Weak-reference family (WeakRef + WeakMap + WeakSet) substrate
//! for the torajs AOT TypeScript runtime.
//!
//! Layer-3 substrate of the architecture rewrite (`docs/architecture-
//! rewrite.md` P4.3'). Houses three observed-reference containers
//! that share a single process-global target → observer registry:
//!
//! - `WeakRef<T>` — 16-byte heap struct: universal header + `target`
//!   pointer (NULL after target reclamation). `wr.deref()` returns
//!   the target with strong rc bumped, or null.
//! - `WeakMap<K, V>` (P4.3'-c, pending) — keys are observed; entries
//!   auto-evict when the key's strong rc transitions to zero.
//! - `WeakSet<K>` (P4.3'-d, pending) — same shape, value-less.
//!
//! ## Why split from torajs-collections
//!
//! Per [[project-status-2026-05-23]] L3a + handoff design notes:
//! weak observation is semantically distinct from strong ownership,
//! and the registry is **process-global** (not per-container) — every
//! WeakRef / WeakMap / WeakSet hooks into the same `g_buckets[1024]`
//! cell table keyed by target pointer, so a single dying-target walk
//! broadcasts cleanup across all observer kinds in one pass. A clean
//! crate boundary keeps strong-ref collection invariants from leaking
//! into the weak-ref / registry path.
//!
//! ## Sub-step matrix (P4.3')
//!
//! | Phase     | Adds                                                  |
//! |-----------|-------------------------------------------------------|
//! | P4.3'-a   | scaffold + `WeakRef` layout + create/deref/drop       |
//! | P4.3'-b   | WeakRef remaining ops + registry port to Rust         |
//! | P4.3'-c   | WeakMap port (~253 C LOC)                             |
//! | P4.3'-d   | WeakSet port (~176 C LOC)                             |
//! | P4.3'-e   | shared registry consolidation + nuke 3 runtime_weak*.c|
//!
//! ## What's in C still (P4.3'-a)
//!
//! For this first cut only the three owner-side WeakRef fns
//! (`create` / `deref` / `drop`) are in Rust. The shared registry
//! (`g_buckets`, `registry_register`, `registry_deregister`,
//! `target_dying`) remains in `runtime_weakref.c` — Rust calls into
//! it via `extern "C"`. P4.3'-b consolidates the registry into Rust.
//!
//! ## Why `std`, not `no_std`
//!
//! Same reason as torajs-rc / str / num / bigint / arr / dynobj /
//! collections — cargo `cargo test` + dual `crate-type = ["rlib",
//! "staticlib"]` + `no_std` trips a precompiled-core panic-strategy
//! mismatch on stable. `std` staticlibs link cleanly at `tr build`
//! time.

pub mod create;
pub mod deref;
pub mod drop;
pub mod layout;
pub mod registry;
pub mod weakmap;
pub mod weakset;

pub use create::__torajs_weakref_create;
pub use deref::__torajs_weakref_deref;
pub use drop::__torajs_weakref_drop;
pub use registry::{
    __torajs_weakref_registry_deregister, __torajs_weakref_registry_register,
    __torajs_weakref_target_dying,
};
pub use weakmap::{
    __torajs_weakmap_create, __torajs_weakmap_delete, __torajs_weakmap_drop, __torajs_weakmap_get,
    __torajs_weakmap_has, __torajs_weakmap_invalidate_key, __torajs_weakmap_set,
};
pub use weakset::{
    __torajs_weakset_add, __torajs_weakset_create, __torajs_weakset_delete, __torajs_weakset_drop,
    __torajs_weakset_has, __torajs_weakset_invalidate_key,
};

// Cross-tier extern stubs for cargo unit tests — the real symbols
// for rc + invalidate_key live in libs / C files that the test binary
// doesn't link. registry register/deregister/target_dying are NOW
// provided by this crate (P4.3'-b) so they no longer need stubs.
//
// `__torajs_weakmap_invalidate_key` / `__torajs_weakset_invalidate_key`
// come from `runtime_weakmap.c` / `runtime_weakset.c` at `tr build`
// link time — stubbed here. Will be removed when P4.3'-c / P4.3'-d
// port those into this crate.
#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_rc_inc(_p: *mut core::ffi::c_void) {
    panic!(
        "torajs-weak unit-test stub: __torajs_rc_inc should not be called from cargo test paths"
    );
}

// `__torajs_weakmap_invalidate_key` + `__torajs_weakset_invalidate_key`
// are now provided by `weakmap` / `weakset` modules (P4.3'-c / -d) —
// no stubs needed.

// `__torajs_value_drop_heap` comes from `runtime_str.c` at `tr build`
// link time — stubbed for cargo test (used by weakmap::set/delete/
// invalidate_key/drop paths).
#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_value_drop_heap(_p: *mut core::ffi::c_void) {
    panic!(
        "torajs-weak unit-test stub: __torajs_value_drop_heap should not be called from cargo test paths"
    );
}
