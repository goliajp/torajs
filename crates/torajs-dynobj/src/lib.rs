//! Dynamic-property object substrate for the torajs AOT TypeScript
//! runtime.
//!
//! Layer-3 substrate of the architecture rewrite (`docs/architecture-
//! rewrite.md` P4.2). Open-addressing hashmap backing `obj.x = v` /
//! `arr.x = v` / `fn.x = v` property bags. FNV-1a string-keyed buckets,
//! linear probe, load-factor `(count + tomb) > cap * 7/8` → cap doubles.
//!
//! Layout (mirrors `runtime_str.c` 1:1 — the C-side keeps macro-form
//! offsets for in-file callers; same contract, separately compiled):
//!
//! ```text
//! offset 0  : universal heap header (8B; refcount/type_tag/flags)
//! offset 8  : count (u32) — # of live entries
//! offset 12 : cap   (u32) — bucket array size (power of 2)
//! offset 16 : tomb  (u32) — # of tombstone slots
//! offset 20 : pad   (u32)
//! offset 24 : buckets[cap] of `{ key_ptr:*Str, tag:u64, value:u64 }` (24B each)
//! ```
//!
//! Reference: Swift Dictionary / CPython compact dict open-addressing.
//! Self-implemented per CLAUDE.md "自研" pillar (no external hash lib).
//!
//! ## Sub-step matrix (P4.2)
//!
//! | Phase  | Adds                                                |
//! |--------|-----------------------------------------------------|
//! | P4.2-a | scaffold + `__torajs_dynobj_alloc`                  |
//! | P4.2-b | probe / hash_str / str_eq helpers (Rust internals)  |
//! | P4.2-c | get_tag / get_value / get_flags                     |
//! | P4.2-d | set + resize                                        |
//! | P4.2-e | define (attribute-flag tracking)                    |
//! | P4.2-f | has / delete / drop — remove last C body            |
//!
//! ## Why `std`, not `no_std`
//!
//! Same reason as the rest of the Layer-1+ Rust sub-crates: cargo's
//! `cargo test` + dual `crate-type = ["rlib", "staticlib"]` + `no_std`
//! combo trips a precompiled-core panic-strategy mismatch on stable.
//! `std` staticlibs link cleanly at `tr build` time.

pub mod alloc;
pub mod define;
pub mod delete;
pub mod drop;
pub mod get;
pub mod has;
pub mod layout;
pub mod probe;
pub mod resize;
pub mod set;

pub use alloc::__torajs_dynobj_alloc;
pub use define::__torajs_dynobj_define;
pub use delete::__torajs_dynobj_delete;
pub use drop::__torajs_dynobj_drop;
pub use get::{__torajs_dynobj_get_flags, __torajs_dynobj_get_tag, __torajs_dynobj_get_value};
pub use has::__torajs_dynobj_has;
pub use set::__torajs_dynobj_set;

// Cross-tier extern stubs for cargo unit tests — `__torajs_rc_inc`,
// `__torajs_throw_type_error`, and `__torajs_value_drop_heap` are
// provided by their respective libtorajs_*.a at `tr build` link time;
// stubs here let the test binary link cleanly. Same pattern as
// torajs-arr's `__torajs_throw_range_error` / `__torajs_str_alloc_pooled`
// test stubs.
#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_rc_inc(_p: *mut core::ffi::c_void) {
    panic!(
        "torajs-dynobj unit-test stub: __torajs_rc_inc should not be called from cargo test paths"
    );
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_throw_type_error(_msg: *const u8) {
    panic!(
        "torajs-dynobj unit-test stub: __torajs_throw_type_error should not be called from cargo test paths"
    );
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_value_drop_heap(_child: *mut core::ffi::c_void) {
    panic!(
        "torajs-dynobj unit-test stub: __torajs_value_drop_heap should not be called from cargo test paths"
    );
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_rc_dec(_p: *mut core::ffi::c_void) -> i32 {
    panic!(
        "torajs-dynobj unit-test stub: __torajs_rc_dec should not be called from cargo test paths"
    );
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_drop(_s: *mut core::ffi::c_void) {
    panic!(
        "torajs-dynobj unit-test stub: __torajs_str_drop should not be called from cargo test paths"
    );
}
