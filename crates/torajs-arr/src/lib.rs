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

// v0.7-A2 step 6b — force-link mmalloc.
extern crate torajs_mmalloc as _;

pub mod alloc;
pub mod any;
pub mod any_fill;
pub mod any_get;
pub mod any_to_typed;
pub mod any_typed_bridge;
pub mod define;
pub mod define_accessor;
pub mod define_hole;
pub mod define_index;
pub mod define_index_flags;
pub mod define_length;
pub mod drop;
pub mod from_string;
pub mod grow;
pub mod grow_store;
pub mod index_any;
pub mod iter;
pub mod join;
mod join_enc;
pub mod join_locale;
pub mod layout;
pub mod mark_kind;
pub mod method_any;
pub mod method_any_copy;
pub mod method_any_hof;
pub mod method_any_search;
pub mod method_any_transform;
pub mod null_guard;
pub mod ops;
pub mod pool;
pub mod print;
pub mod print_any;
pub mod print_inline;
mod print_props;
mod print_typed;
pub mod props;
pub mod reverse_mop;
pub mod slice;
pub mod sort;
pub mod species;
pub mod str_bridge;
pub mod subclass_alloc;
pub mod sum_precise;
pub mod throw_empty;
pub mod transform;
pub mod transform_splice;

pub use alloc::{
    __torajs_arr_alloc, __torajs_arr_alloc_any, __torajs_arr_alloc_any_filled,
    __torajs_arr_alloc_any_filled_f64, __torajs_arr_alloc_pooled, __torajs_arr_free,
};
pub use any::{
    __torajs_arr_extend_any, __torajs_arr_flat_any, __torajs_arr_get_any_boxed,
    __torajs_arr_get_any_tag, __torajs_arr_get_any_value, __torajs_arr_push_any,
    __torajs_arr_set_any,
};
pub use any_typed_bridge::__torajs_arr_extend_typed_into_any;
pub use drop::{__torajs_arr_drop, __torajs_arr_drop_any, __torajs_arr_drop_heap};
pub use from_string::__torajs_arr_from_string;
pub use grow::{
    __torajs_arr_push, __torajs_arr_reserve, __torajs_arr_set_length_truncate_scalar,
    __torajs_arr_set_length_validate, __torajs_arr_shift,
};
pub use iter::{
    __torajs_arr_iter_create_entries, __torajs_arr_iter_create_keys,
    __torajs_arr_iter_create_values, __torajs_arr_iter_drop, __torajs_arr_iter_step,
};
pub use join::{
    __torajs_arr_join, __torajs_arr_join_bool, __torajs_arr_join_f64, __torajs_arr_join_i64,
    __torajs_arr_join_substr,
};
pub use join_locale::{__torajs_arr_join_f64_locale, __torajs_arr_join_i64_locale};
pub use ops::{__torajs_arr_extend_unchecked, __torajs_arr_push_unchecked};
pub use print::{
    __torajs_arr_print_bool, __torajs_arr_print_f64, __torajs_arr_print_i64,
    __torajs_arr_print_str, __torajs_arr_print_substr,
};
pub use slice::__torajs_arr_slice;
pub use sort::__torajs_arr_sort_cb;
pub use throw_empty::{__torajs_arr_throw_reduce_empty, __torajs_arr_throw_reduce_right_empty};
pub use transform::{
    __torajs_arr_concat, __torajs_arr_copy_within, __torajs_arr_fill, __torajs_arr_flat,
    __torajs_arr_reverse, __torajs_arr_to_reversed, __torajs_arr_unshift, __torajs_arr_with,
};
pub use transform_splice::{__torajs_arr_splice, __torajs_arr_splice_items};

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

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_alloc_pooled_enc(_len: u64, _is_latin1: i64) -> *mut u8 {
    panic!(
        "torajs-arr unit-test stub: __torajs_str_alloc_pooled_enc should not be called from cargo test paths"
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

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_throw_type_error(_msg: *const core::ffi::c_char) {
    panic!(
        "torajs-arr unit-test stub: __torajs_throw_type_error should not be called from cargo test paths"
    );
}

// Iter externs (P4.3-g): `__torajs_rc_inc` / `__torajs_rc_dec` come
// from the `torajs-rc` rlib dep — NO test stub for them here, because
// stubbing alongside the real definition triggers the LTO=fat
// release-mode "Linking globals named '__torajs_rc_dec': symbol
// multiply defined!" failure (same applies to rc_inc). Pattern
// matches torajs-collections (which has no torajs-rc dep and so does
// stub them).
//
// `__torajs_value_drop_heap` lives in `torajs_rc::drop_dispatch`
// at production link time (libtorajs_rc.a in the tr build). The
// stub below is needed for the cargo test binary — without it the
// linker errors with `Undefined symbols` because torajs-arr's lib
// declares `extern "C" { __torajs_value_drop_heap }` for the
// any-slot drop path, and torajs-rc's rlib doesn't unconditionally
// expose the symbol (no Rust call site referencing it triggers
// DCE in the rlib path).
#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_value_drop_heap(_p: *mut core::ffi::c_void) {
    panic!(
        "torajs-arr unit-test stub: __torajs_value_drop_heap should not be called from cargo test paths"
    );
}

// `__torajs_str_sort_undef_pre` (called from sort.rs's is_gt when
// mode bit 3 = Str elements) lives in libtorajs_str.a at `tr build`
// link time; stubbed for cargo test (no sort unit test sets bit 3).
#[cfg(test)]
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_str_sort_undef_pre(_a: *const u8, _b: *const u8) -> i64 {
    panic!(
        "torajs-arr unit-test stub: __torajs_str_sort_undef_pre should not be called from cargo test paths"
    );
}

// The ANY sort modes (sort.rs is_gt, backfill chunk 4) reference
// the anyvalue NaN-box protocol + the str default comparator; all
// live in their staticlibs at `tr build` link time. No sort unit
// test sets the ANY bits — panic stubs keep misuse loud.
#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_box_from_pair(_t: i64, _v: i64) -> u64 {
    panic!("torajs-arr unit-test stub: __torajs_anyv_box_from_pair (ANY sort mode) unexpected");
}
#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_unbox_tag(_v: u64) -> i64 {
    panic!("torajs-arr unit-test stub: __torajs_anyv_unbox_tag (ANY sort mode) unexpected");
}
#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_to_number(_v: u64) -> f64 {
    panic!("torajs-arr unit-test stub: __torajs_anyv_to_number (ANY sort mode) unexpected");
}
#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_to_str(_v: u64) -> *mut core::ffi::c_void {
    panic!("torajs-arr unit-test stub: __torajs_anyv_to_str (ANY sort mode) unexpected");
}
#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_sort_cmp(_a: *const u8, _b: *const u8) -> i64 {
    panic!("torajs-arr unit-test stub: __torajs_str_sort_cmp (ANY sort mode) unexpected");
}
#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_drop(_s: *mut core::ffi::c_void) {
    panic!("torajs-arr unit-test stub: __torajs_str_drop (ANY sort mode) unexpected");
}

// `__torajs_split_block_free_push` (called from `__torajs_arr_free`
// in alloc.rs) is defined in runtime_str.c at `tr build` link time;
// no rlib provider, so a stub is required for cargo test linking.
#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_split_block_free_push(_p: *mut u8) -> i32 {
    panic!(
        "torajs-arr unit-test stub: __torajs_split_block_free_push should not be called from cargo test paths"
    );
}

// `__torajs_cycle_buffer` / `__torajs_cycle_unbuffer` (called from
// drop.rs's cycle-root hooks, chunk 614) live in libtorajs_cycle.a
// at `tr build` link time; stubbed for cargo test. The NULL-drop
// unit tests never reach them (NULL early-outs precede the rc dec).
#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_cycle_buffer(_p: *mut core::ffi::c_void) {
    panic!(
        "torajs-arr unit-test stub: __torajs_cycle_buffer should not be called from cargo test paths"
    );
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_cycle_unbuffer(_p: *mut core::ffi::c_void) {
    panic!(
        "torajs-arr unit-test stub: __torajs_cycle_unbuffer should not be called from cargo test paths"
    );
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_subclass_drop_entry(_p: *mut core::ffi::c_void) {
    panic!(
        "torajs-arr unit-test stub: __torajs_subclass_drop_entry should not be called from cargo test paths"
    );
}
