//! Default resolution of the link-judged seam on the `in` walk of a
//! struct cell that lacks a declared member (see [`super::link_seams`]
//! for the seam contract).

unsafe extern "C" {
    fn __torajs_struct_lacks_in_user_proto_impl(
        props: *const core::ffi::c_void,
        key: *const core::ffi::c_void,
    ) -> i64;
}

/// Default resolution of HasProperty along a re-parented struct cell's
/// user [[Prototype]] (torajs-anyvalue). The link judgment stubs it
/// (fam 64) when nothing can re-parent a struct: the one writer of
/// `FLAG_OBJ_USER_PROTO`, `__torajs_struct_user_proto_raise`, is dead.
///
/// # Safety
/// `props` is the expando dict of a cell with `FLAG_OBJ_USER_PROTO`
/// raised; `key` is a live Str cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_struct_lacks_in_user_proto_slow(
    props: *const core::ffi::c_void,
    key: *const core::ffi::c_void,
) -> i64 {
    unsafe { __torajs_struct_lacks_in_user_proto_impl(props, key) }
}
