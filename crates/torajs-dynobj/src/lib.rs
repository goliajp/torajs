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
//! Implemented in-house: the runtime takes no external hash library.
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
pub mod accessor_invoke;
pub mod alloc;
pub mod attach_exec;
#[cfg(test)]
mod builtin_proto_stubs;
pub mod define;
pub mod define_all;
mod define_bag;
pub mod define_entry;
pub mod define_expando;
mod define_from_desc;
pub mod define_redefine;
pub mod define_struct;
mod define_struct_attrs;
mod define_struct_shadow;
mod define_typedarray;
pub mod define_wrapper;
pub mod delete;
pub mod dict;
pub mod drop;
pub mod get;
pub mod global_cell;
pub mod has;
pub mod iter;
pub mod iter_print_order;
pub mod iter_slow_mode;
mod key_wtf8;
pub mod layout;
pub mod move_to_end;
mod nanbox;
pub mod pool;
pub mod print_any;
pub mod probe;
mod probe_str;
pub mod proto_chain;
mod proto_note;
pub mod resize;
pub mod seal;
pub mod set;
pub mod set_entry_flags;

pub use accessor::{
    __torajs_accessor_drop, __torajs_accessor_get_getter, __torajs_accessor_get_kinds,
    __torajs_accessor_get_setter, __torajs_accessor_invoke_getter, __torajs_accessor_invoke_setter,
    __torajs_accessor_pair_new,
};
pub use alloc::{
    __torajs_dynobj_alloc, __torajs_dynobj_mark_class_ctor, __torajs_dynobj_mark_module_ns,
    __torajs_dynobj_mark_null_proto,
};
pub use define::__torajs_dynobj_define;
pub use delete::__torajs_dynobj_delete;
pub use drop::__torajs_dynobj_drop;
pub use get::{
    __torajs_dynobj_get_flags, __torajs_dynobj_get_own, __torajs_dynobj_get_tag,
    __torajs_dynobj_get_value,
};
pub use has::__torajs_dynobj_has;
pub use iter::{
    __torajs_dynobj_iter_flags, __torajs_dynobj_iter_key, __torajs_dynobj_iter_len,
    __torajs_dynobj_iter_order, __torajs_dynobj_iter_value,
};
pub use seal::{
    __torajs_dynobj_all_entries_non_configurable, __torajs_dynobj_freeze_entries,
    __torajs_dynobj_lock_builtin_fn_class_slots, __torajs_dynobj_seal_entries,
};
pub use set::__torajs_dynobj_set;

// Cross-tier extern stubs for cargo unit tests — `__torajs_rc_inc`,
// `__torajs_throw_type_error`, and `__torajs_value_drop_heap` are
// provided by their respective libtorajs_*.a at `tr build` link time;
// stubs here let the test binary link cleanly. Same pattern as
// torajs-arr's `__torajs_throw_range_error` / `__torajs_str_alloc_pooled`
// test stubs.
/// # Safety
/// Never called; any call panics.
#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_rc_inc(_p: *mut core::ffi::c_void) {
    panic!(
        "torajs-dynobj unit-test stub: __torajs_rc_inc should not be called from cargo test paths"
    );
}

/// # Safety
/// Never called; any call panics.
#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_throw_reference_error_tdz(_name: *mut core::ffi::c_void) {
    panic!(
        "torajs-dynobj unit-test stub: __torajs_throw_reference_error_tdz should not be called \
         from cargo test paths"
    );
}

/// # Safety
/// Never called; any call panics.
#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_regex_last_index_define(
    _re: *mut core::ffi::c_void,
    _tag: u64,
    _value: u64,
    _flags: u64,
) -> i64 {
    panic!(
        "torajs-dynobj test stub: __torajs_regex_last_index_define should not be called from cargo test (no RegExp receiver is built there)"
    )
}

// Rotation 652 — the generic container / descriptor arms of the
// defineProperties walk reach two more anyvalue / meta kernels; no
// unit test builds a Date-shaped descriptor, so these panic like the
// stubs above.
/// # Safety
/// Never called; any call panics.
#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_own_keys(
    _v: u64,
    _include_nonenum: i64,
) -> *mut core::ffi::c_void {
    panic!(
        "torajs-dynobj unit-test stub: __torajs_anyv_own_keys should not be called from cargo test paths"
    );
}

/// # Safety
/// Never called; any call panics.
#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_cell_expando_bag(
    _p: *mut core::ffi::c_void,
) -> *mut core::ffi::c_void {
    panic!(
        "torajs-dynobj unit-test stub: __torajs_cell_expando_bag should not be called from cargo test paths"
    );
}

/// # Safety
/// Never called; any call panics.
#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_throw_type_error(_msg: *const u8) {
    panic!(
        "torajs-dynobj unit-test stub: __torajs_throw_type_error should not be called from cargo test paths"
    );
}

// Faithful Closure-arm double for unit tests (mirrors the faithful
// rc_dec stub below): `accessor::tests` legitimately exercises the
// pair teardown, which routes each held closure ref through this
// dispatcher (rc-gated dec, drop_fn at +16 on the zero transition).
// Only the Closure shape is emulated — the accessor tests are the
// sole unit-test caller; anything else reaching here is still a
// contract break worth failing loudly on, which the drop_fn == 0
// fall-through below surfaces as a no-op the asserting test catches.
/// # Safety
/// `child` is NULL or a live test cell: a u32 refcount at +0 and, at +16, either 0 or an `extern "C" fn(*mut c_void)` drop hook.
#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_value_drop_heap(child: *mut core::ffi::c_void) {
    if child.is_null() {
        return;
    }
    let rc = child as *mut u32;
    unsafe { *rc -= 1 };
    if unsafe { *rc } == 0 {
        let drop_fn = unsafe { *((child as *const u8).add(16) as *const usize) };
        if drop_fn != 0 {
            let f: unsafe extern "C" fn(*mut core::ffi::c_void) =
                unsafe { core::mem::transmute(drop_fn) };
            unsafe { f(child) };
        }
    }
}

// Faithful refcount-dec stub for unit tests: the real torajs-rc dec
// (universal header refcount at +0) is linked at `tr build` time, but
// the `accessor::tests` drop path legitimately exercises dec here, so
// the stub mirrors the real semantics — decrement the header u32,
// return 1 on the transition to zero (caller should free), 0 otherwise.
/// # Safety
/// `p` is NULL or points at a live u32 refcount.
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

/// # Safety
/// Never called; any call panics.
#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_wtf8_into(_s: *const u8, _buf: *mut u8, _cap: u32) -> u32 {
    panic!(
        "torajs-dynobj unit-test stub: __torajs_str_wtf8_into should not be called from cargo test paths"
    );
}

/// # Safety
/// Never called; any call panics.
#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_drop(_s: *mut core::ffi::c_void) {
    panic!(
        "torajs-dynobj unit-test stub: __torajs_str_drop should not be called from cargo test paths"
    );
}

/// # Safety
/// Never called; any call panics.
#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_symbol_drop(_s: *mut core::ffi::c_void) {
    panic!(
        "torajs-dynobj unit-test stub: __torajs_symbol_drop should not be called from cargo test paths"
    );
}

// The owner's encode / decode for unit tests: the reference encoding
// `crate::nanbox` holds its inline steps to, so a value written on one
// path reads back on the other.
/// # Safety
/// Takes no pointer arguments.
#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64 {
    nanbox::reference::box_from_pair(tag, value)
}

/// # Safety
/// Takes no pointer arguments.
#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_unbox_tag(v: u64) -> i64 {
    nanbox::reference::unbox_tag(v)
}

#[cfg(test)]
thread_local! {
    /// How many ShortStr boxes the value decode double below was asked
    /// to materialize on this thread: the owner mints a fresh `Str`
    /// for each, which its caller has to release.
    pub(crate) static SHORT_STR_MATERIALIZED: core::cell::Cell<usize> =
        const { core::cell::Cell::new(0) };
}

/// A ShortStr is counted and answers a stand-in pointer; every other
/// box decodes as the owner does.
///
/// # Safety
/// Takes no pointer arguments.
#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_unbox_value(v: u64) -> i64 {
    if nanbox::reference::is_short_str(v) {
        SHORT_STR_MATERIALIZED.with(|n| n.set(n.get() + 1));
        return 0x1000;
    }
    nanbox::reference::unbox_value(v)
}

// Accessor-pair builtin-face probe stubs — unit tests exercise
// ordinary-closure faces only, so the probe always answers "not a
// builtin cell" and the dispatch is unreachable.
/// # Safety
/// Test stub; dereferences none of its arguments.
#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_builtin_method_face_mid(_p: *const core::ffi::c_void) -> i64 {
    -1
}

/// # Safety
/// Never called; any call panics.
#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_builtin_method_face_dispatch(
    _recv: u64,
    _mid: i64,
    _argv: *const u64,
    _argc: i64,
) -> u64 {
    panic!("torajs-dynobj unit-test stub: builtin-face dispatch should not run under cargo test")
}

/// # Safety
/// Test stub; dereferences none of its arguments.
#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_class_face_adapter(_p: *const core::ffi::c_void) -> u64 {
    0
}

/// # Safety
/// Never called; any call panics.
#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_class_face_invoke(
    _adapter: u64,
    _recv: u64,
    _argv: *const u64,
    _argc: i64,
) -> u64 {
    panic!("torajs-dynobj unit-test stub: class-face invoke should not run under cargo test")
}

/// # Safety
/// Test stub; dereferences none of its arguments.
#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_class_accessor_face_invoke(
    _face: *const core::ffi::c_void,
    _recv: u64,
    _argv: *const u64,
    _argc: i64,
) -> u64 {
    0
}

// The own-entry write tests reach the setter dispatch; none of them
// stores an accessor, so the closure entry read never runs.
/// # Safety
/// Never called; any call panics.
#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_closure_boxed_entry(_cell: *const u8) -> u64 {
    panic!("torajs-dynobj unit-test stub: no unit test stores an accessor closure")
}

/// # Safety
/// Never called; any call panics.
#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_struct_put_own_rows_at(
    _cell: *const core::ffi::c_void,
    _indent: u32,
    _nearer: *const *const core::ffi::c_void,
    _nearer_len: usize,
    _any_emitted: *mut i32,
) {
    panic!("torajs-dynobj unit-test stub: struct rows should not print under cargo test")
}

/// # Safety
/// Test stub; dereferences none of its arguments.
#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_class_object_print(_cell: *const core::ffi::c_void) -> i32 {
    0
}
