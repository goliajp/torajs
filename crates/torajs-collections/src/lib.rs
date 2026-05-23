//! Strong-ref collections substrate for the torajs AOT TypeScript runtime.
//!
//! Layer-3 substrate of the architecture rewrite (`docs/architecture-
//! rewrite.md` P4.3). Map<K,V> + Set<K> share a single `Map` struct
//! (Set is an SSA-side distinction — storage shape is identical); both
//! wear `TAG_MAP = 15` at the heap header level.
//!
//! ## Why two arrays
//!
//! JavaScript's `Map.prototype[@@iterator]` must yield entries in
//! **insertion order** (spec §24.1.5.1). A pure in-place hashbucket
//! layout (like `torajs-dynobj`) loses that order on probe. So the
//! Map struct holds:
//!
//! - `entries[]` — packed `MapEntry`s appended in insertion order;
//!   source of truth for set / get / has / delete. Live entries have
//!   `hash >= 1`; deletes mark `hash = 0` (ENTRY_HASH_TOMBSTONE) so
//!   iter walks can skip them without touching `slots[]`.
//! - `slots[]` — 64-bit hash-table cells `(hash:hi32, entry_idx:lo32)`,
//!   robin-hood probed. Pure acceleration index; rebuilt on rehash.
//!   Entry indices into `entries[]` are stable across reshuffle, so
//!   iteration ordering survives any number of slot-side swaps.
//!
//! ## SameValueZero key equality
//!
//! Per ES spec §7.2.10: `NaN === NaN` (all NaN bit patterns collide
//! into one canonical hash); `+0 === -0` (the IEEE eq predicate
//! already says so); strings compare byte-by-byte; non-Str heap
//! objects compare by pointer identity. Implemented in
//! [`crate::eq::map_keys_equal`] (P4.3-b).
//!
//! ## Sub-step matrix (P4.3)
//!
//! | Phase    | Adds                                                    |
//! |----------|---------------------------------------------------------|
//! | P4.3-a   | scaffold + layout + `__torajs_map_create`               |
//! | P4.3-b   | `hash` + `eq` internals (FNV-1a + SplitMix + SameValueZero) |
//! | P4.3-c   | `probe` internals (robin-hood insert + lookup + rehash) |
//! | P4.3-d   | `__torajs_map_size` / `_has` / `_get`                   |
//! | P4.3-e   | `__torajs_map_set`                                      |
//! | P4.3-f   | `__torajs_map_delete` / `_clear`                        |
//! | P4.3-g   | `__torajs_map_drop` (P4.3 Map closer)                   |
//! | P4.3-h   | `MapIter` family                                        |
//! | P4.3-i   | ArrIter port to `torajs-arr::iter` (was misplaced in C  |
//! |          | runtime_map.c; P4.1 missed it)                          |
//!
//! ## Why `std`, not `no_std`
//!
//! Same reason as torajs-rc / str / num / bigint / arr / dynobj —
//! cargo `cargo test` + dual `crate-type = ["rlib", "staticlib"]` +
//! `no_std` trips a precompiled-core panic-strategy mismatch on
//! stable. `std` staticlibs link cleanly at `tr build` time.

pub mod create;
pub mod delete;
pub mod drop;
pub mod eq;
pub mod hash;
pub mod iter;
pub mod layout;
pub mod mutate;
pub mod probe;
pub mod query;

pub use create::__torajs_map_create;
pub use delete::{__torajs_map_clear, __torajs_map_delete};
pub use drop::__torajs_map_drop;
pub use iter::{
    __torajs_map_iter_create_entries, __torajs_map_iter_create_keys,
    __torajs_map_iter_create_set_entries, __torajs_map_iter_create_values, __torajs_map_iter_drop,
    __torajs_map_iter_next, __torajs_map_iter_step,
};
pub use mutate::__torajs_map_set;
pub use query::{__torajs_map_get, __torajs_map_has, __torajs_map_size};

// Cross-tier extern stubs for cargo unit tests — the real symbols
// (__torajs_str_eq / __torajs_rc_inc / __torajs_value_drop_heap) are
// provided by their respective lib*.a at `tr build` link time; the
// test binary doesn't link those, so panicking stubs keep cargo test
// linking clean. Same pattern as torajs-arr / torajs-dynobj test stubs.
#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_eq(_a: *const u8, _b: *const u8) -> i64 {
    panic!(
        "torajs-collections unit-test stub: __torajs_str_eq should not be called from cargo test paths"
    );
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_rc_inc(_p: *mut core::ffi::c_void) {
    panic!(
        "torajs-collections unit-test stub: __torajs_rc_inc should not be called from cargo test paths"
    );
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_value_drop_heap(_p: *mut core::ffi::c_void) {
    panic!(
        "torajs-collections unit-test stub: __torajs_value_drop_heap should not be called from cargo test paths"
    );
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_rc_dec(_p: *mut core::ffi::c_void) -> i32 {
    panic!(
        "torajs-collections unit-test stub: __torajs_rc_dec should not be called from cargo test paths"
    );
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_alloc_any(_cap: u64) -> *mut core::ffi::c_void {
    panic!(
        "torajs-collections unit-test stub: __torajs_arr_alloc_any should not be called from cargo test paths"
    );
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_push_any(
    _arr: *mut core::ffi::c_void,
    _tag: u64,
    _value: u64,
) -> *mut core::ffi::c_void {
    panic!(
        "torajs-collections unit-test stub: __torajs_arr_push_any should not be called from cargo test paths"
    );
}
