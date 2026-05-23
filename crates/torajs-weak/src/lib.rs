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

pub use create::__torajs_weakref_create;
pub use deref::__torajs_weakref_deref;
pub use drop::__torajs_weakref_drop;

// Cross-tier extern stubs for cargo unit tests — the real symbols
// (__torajs_rc_inc / __torajs_weakref_registry_register /
// __torajs_weakref_registry_deregister) are provided by their
// respective lib*.a / runtime_weakref.c.o at `tr build` link time;
// the test binary doesn't link those, so panicking stubs keep cargo
// test linking clean. Same pattern as torajs-arr / torajs-collections
// test stubs.
#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_rc_inc(_p: *mut core::ffi::c_void) {
    panic!(
        "torajs-weak unit-test stub: __torajs_rc_inc should not be called from cargo test paths"
    );
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_weakref_registry_register(
    _target: *mut core::ffi::c_void,
    _kind: u32,
    _owner: *mut core::ffi::c_void,
) {
    panic!(
        "torajs-weak unit-test stub: __torajs_weakref_registry_register should not be called from cargo test paths"
    );
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_weakref_registry_deregister(
    _target: *mut core::ffi::c_void,
    _kind: u32,
    _owner: *mut core::ffi::c_void,
) {
    panic!(
        "torajs-weak unit-test stub: __torajs_weakref_registry_deregister should not be called from cargo test paths"
    );
}
