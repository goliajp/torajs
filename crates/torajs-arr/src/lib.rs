//! Array<T> + Array<Any> substrate for the torajs AOT TypeScript
//! runtime.
//!
//! Layer-3 substrate of the architecture rewrite (`docs/architecture-
//! rewrite.md` P4.1). Heap-allocated dynamic array with a refcounted
//! universal heap header + `len` + `cap` + `slots[]`. Two sub-flavors
//! (selected by `type_tag` + `FLAG_ARR_ANY`):
//!
//! - `Array<T>` — slots are 8-byte raw values (i64 / f64 / Str ptr / ...)
//! - `Array<Any>` — slots are 16-byte tag/value pairs (boxed-Any)
//!
//! Pool-aware free — small-cap blocks (`cap ≤ ARR_POOL_PAYLOAD`) return
//! to a thread-local LIFO pool; large blocks go straight to libc free.
//! The pool itself lives in C (`runtime_str.c::arr_pool_*`) for now —
//! P4.1+ ships ports of each public fn over time.
//!
//! ## Sub-step matrix (P4.1)
//!
//! | Phase   | Adds                                                |
//! |---------|-----------------------------------------------------|
//! | P4.1-a  | scaffold + ArrHeader layout + `__torajs_arr_drop`   |
//! | P4.1-b  | basic ops: push / pop / get / set / len / alloc     |
//! | P4.1-c  | iter (forEach/map/filter/reduce + ArrIter struct)   |
//! | P4.1-d  | slice / concat / join / sort / reverse              |
//! | ...     | (continued — Array surface is large; one family / step) |
//!
//! ## Why `std`, not `no_std`
//!
//! Same reason as torajs-rc / torajs-str / torajs-num / torajs-bigint:
//! cargo's `cargo test` + dual `crate-type = ["rlib", "staticlib"]`
//! + `no_std` combination triggers a precompiled-core panic-strategy
//! mismatch with no clean fix on stable. `std` staticlibs link cleanly
//! at `tr build` time.

pub mod alloc;
pub mod any;
pub mod drop;
pub mod grow;
pub mod join;
pub mod layout;
pub mod ops;
pub mod pool;
pub mod print;
pub mod props;
pub mod slice;
pub mod str_bridge;

pub use alloc::{__torajs_arr_alloc, __torajs_arr_alloc_pooled, __torajs_arr_free};
pub use any::{
    __torajs_arr_alloc_any, __torajs_arr_alloc_any_filled, __torajs_arr_extend_any,
    __torajs_arr_get_any_tag, __torajs_arr_get_any_value, __torajs_arr_push_any,
    __torajs_arr_set_any,
};
pub use drop::{__torajs_arr_drop, __torajs_arr_drop_any};
pub use grow::{__torajs_arr_reserve, __torajs_arr_set_length_validate};
pub use join::{
    __torajs_arr_join, __torajs_arr_join_bool, __torajs_arr_join_f64, __torajs_arr_join_i64,
    __torajs_arr_join_substr, __torajs_arr_to_reversed, __torajs_arr_with,
};
pub use ops::{__torajs_arr_extend_unchecked, __torajs_arr_push_unchecked};
pub use print::{
    __torajs_arr_print_bool, __torajs_arr_print_f64, __torajs_arr_print_i64,
    __torajs_arr_print_str, __torajs_arr_print_substr,
};
pub use slice::__torajs_arr_slice;

// `__torajs_str_alloc_pooled` is provided by `libtorajs_str.a` at
// `tr build` link time. cargo unit tests don't link torajs-str's
// staticlib — provide a panicking stub so the test binary still links.
// Same pattern as torajs-num / torajs-bigint.
#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_alloc_pooled(_len: u64) -> *mut u8 {
    panic!(
        "torajs-arr unit-test stub: __torajs_str_alloc_pooled should not be called from cargo test paths"
    );
}

// Same pattern for torajs-throw — provided by libtorajs_throw.a at
// `tr build` link time; stubbed for cargo test.
#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_throw_range_error(_msg: *const u8) {
    panic!(
        "torajs-arr unit-test stub: __torajs_throw_range_error should not be called from cargo test paths"
    );
}
