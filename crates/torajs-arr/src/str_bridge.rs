//! Bridge to torajs-str's allocator — extern decl only.
//!
//! torajs-arr is Layer-3 architecturally but reaches into Str layout
//! for `arr.join` / `arr_print_str` etc. Layer-3 → Layer-2 Cargo deps
//! are NOT forbidden by the DAG, but routing through `extern "C"`
//! resolution at link time matches the pattern used by
//! `torajs-num::str_bridge` and `torajs-bigint::str_bridge` and keeps
//! the dep tree symmetric across all Str-consuming sub-crates.
//!
//! At `tr build` / `tr run` link time the symbol resolves against
//! libtorajs_str.a; for `cargo test` we provide a `#[cfg(test)]` stub
//! in `lib.rs` so the test binary still links.

unsafe extern "C" {
    /// `__torajs_str_alloc_pooled(len) -> *mut u8` — provided by
    /// `libtorajs_str.a` at link time. Pool-aware Str alloc (`len ≤
    /// STR_POOL_PAYLOAD = 16` recycles via thread-local LIFO; else
    /// libc malloc). Header init: rc=1, type_tag=TAG_STR, flags=0,
    /// len=arg, body uninitialized (caller writes payload bytes after
    /// `+ STR_DATA_OFF = 16`).
    fn __torajs_str_alloc_pooled(len: u64) -> *mut u8;
}

/// Thin wrapper exposing the cross-tier `__torajs_str_alloc_pooled`
/// to other modules within `torajs-arr` (`join.rs` for the typed
/// `arr.join` family).
#[inline]
pub unsafe fn str_alloc_pooled(len: u64) -> *mut u8 {
    unsafe { __torajs_str_alloc_pooled(len) }
}
