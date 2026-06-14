//! Dynamic-property object substrate for the torajs AOT TypeScript
//! runtime.
//!
//! Compact **insertion-ordered** dict backing `obj.x = v` /
//! `arr.x = v` / `fn.x = v` property bags: a dense entry array
//! appended in insertion order plus a power-of-2 FNV-1a hash index
//! mapping probe slots to entry indices (CPython 3.7 dict / V8
//! property-bag shape). JS property insertion order is observable
//! semantics (property printing / `Object.keys` / `for-in`), which a
//! bare linear-probe table cannot give — see [`layout`] for the block
//! shape and hole/tombstone rules, [`iter`] for the ordered walk.
//!
//! Self-implemented per CLAUDE.md "自研" pillar (no external hash lib).
//!
//! ## Why `std`, not `no_std`
//!
//! Same reason as the rest of the Layer-1+ Rust sub-crates: cargo's
//! `cargo test` + dual `crate-type = ["rlib", "staticlib"]` + `no_std`
//! combo trips a precompiled-core panic-strategy mismatch on stable.
//! `std` staticlibs link cleanly at `tr build` time.

// v0.7-A2 step 6b — force-link mmalloc.
extern crate torajs_mmalloc as _;

pub mod accessor;
pub mod alloc;
pub mod define;
pub mod delete;
pub mod drop;
pub mod get;
pub mod has;
pub mod iter;
pub mod layout;
pub mod print_any;
pub mod probe;
pub mod resize;
pub mod seal;
pub mod set;

pub use accessor::{
    __torajs_accessor_drop, __torajs_accessor_get_getter, __torajs_accessor_get_kinds,
    __torajs_accessor_get_setter, __torajs_accessor_invoke_getter, __torajs_accessor_invoke_setter,
    __torajs_accessor_pair_new,
};
pub use alloc::{__torajs_dynobj_alloc, __torajs_dynobj_mark_null_proto};
pub use define::__torajs_dynobj_define;
pub use delete::__torajs_dynobj_delete;
pub use drop::__torajs_dynobj_drop;
pub use get::{__torajs_dynobj_get_flags, __torajs_dynobj_get_tag, __torajs_dynobj_get_value};
pub use has::__torajs_dynobj_has;
pub use iter::{
    __torajs_dynobj_iter_flags, __torajs_dynobj_iter_key, __torajs_dynobj_iter_len,
    __torajs_dynobj_iter_value,
};
pub use seal::{__torajs_dynobj_all_entries_non_configurable, __torajs_dynobj_seal_entries};
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

// Faithful refcount-dec stub for unit tests: the real torajs-rc dec
// (universal header refcount at +0) is linked at `tr build` time, but
// the `accessor::tests` drop path legitimately exercises dec here, so
// the stub mirrors the real semantics — decrement the header u32,
// return 1 on the transition to zero (caller should free), 0 otherwise.
#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_rc_dec(p: *mut core::ffi::c_void) -> i32 {
    if p.is_null() {
        return 0;
    }
    unsafe {
        let rc = p as *mut u32;
        *rc -= 1;
        i32::from(*rc == 0)
    }
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_drop(_s: *mut core::ffi::c_void) {
    panic!(
        "torajs-dynobj unit-test stub: __torajs_str_drop should not be called from cargo test paths"
    );
}
