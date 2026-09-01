//! Unit-test link stubs for the seam symbols the exotic slow paths
//! call into other runtime staticlibs for (`torajs-dynobj` for the
//! props side table, `torajs-weak` for the weakref death notice
//! `__torajs_rc_dec` posts). `tr build` resolves them from the real
//! `.a` files; `cargo test` links none of those, so the test binary
//! needs a definition to link at all. Same pattern as the
//! `__torajs_str_alloc_pooled` / `__torajs_throw_*` stubs in `lib.rs`:
//! reaching one from a unit test is a bug, so they panic.

use core::ffi::c_void;

macro_rules! unreachable_stub {
    ($name:ident) => {
        panic!(concat!(
            "torajs-arr unit-test stub: ",
            stringify!($name),
            " should not be called from cargo test paths"
        ))
    };
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_alloc() -> *mut c_void {
    unreachable_stub!(__torajs_dynobj_alloc)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_has(_obj: *const c_void, _key: *const c_void) -> i32 {
    unreachable_stub!(__torajs_dynobj_has)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_entry_is_hole(
    _obj: *const c_void,
    _key: *const c_void,
) -> i32 {
    unreachable_stub!(__torajs_dynobj_entry_is_hole)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_get_flags(
    _obj: *const c_void,
    _key: *const c_void,
) -> u64 {
    unreachable_stub!(__torajs_dynobj_get_flags)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_set_entry_flags(
    _obj_slot: *mut *mut c_void,
    _key: *mut c_void,
    _flags: u64,
) {
    unreachable_stub!(__torajs_dynobj_set_entry_flags)
}

/// `__torajs_rc_dec` posts every death here; a unit test that frees
/// a cell must not trip a panic, so this one is a no-op like the
/// stub torajs-rc's own tests use.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_weakref_target_dying(_target: *mut c_void) {}
