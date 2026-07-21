//! Mutator family on a primitive receiver — `Array.prototype.pop /
//! push / shift / unshift / reverse / sort / fill / copyWithin /
//! splice` borrowed onto a bool / number (test262
//! `Array/prototype/*/call-with-boolean` family). §23.1.3:
//! `ToObject(prim)` owns no indexed surface and no `length`, so
//! every mutator runs its `len = 0` shape — the observable universe
//! is the return value (the fresh wrapper is this call's own temp
//! except where the spec answers O itself):
//!
//! - `pop` / `shift` → `undefined`
//! - `push` / `unshift` → the new length = argument count
//! - `reverse` / `sort` / `fill` / `copyWithin` → the wrapper
//!   (`… .call(true) instanceof Boolean`)
//! - `splice` → a fresh empty Array
//!
//! Callback callability keeps the loud check at
//! [`crate::method_call_arraylike::arraylike_empty`]'s level: a
//! non-callable `sort` comparator is a TypeError even at `len = 0`.
//!
//! A proto-singleton OWN `length` (the G2d monkey-patch face)
//! answers `None` — a mutator run against the shared singleton as
//! host would write the singleton (silent-wrong), and the
//! wrapper-host walk is not built; the caller's no-such exit stays
//! loud there.

use core::ffi::c_void;

use torajs_rc::{
    ANY_METHOD_COPY_WITHIN, ANY_METHOD_FILL, ANY_METHOD_POP, ANY_METHOD_PUSH, ANY_METHOD_REVERSE,
    ANY_METHOD_SHIFT, ANY_METHOD_SORT, ANY_METHOD_SPLICE, ANY_METHOD_UNSHIFT,
};

use crate::nanbox::{AnyValue, VALUE_UNDEFINED, is_undefined};

unsafe extern "C" {
    /// torajs-arr — fresh Array<Any> (the splice product).
    fn __torajs_arr_alloc_any(cap: u64) -> *mut u8;
    /// torajs-str — key mint for the proto own-length probe.
    fn __torajs_str_alloc(bytes: *const u8, len: i64) -> *mut u8;
    fn __torajs_str_drop(s: *mut c_void);
    /// torajs-dynobj — own-property probe on the proto singleton.
    fn __torajs_dynobj_has(obj: *const c_void, key: *const c_void) -> i32;
    fn __torajs_anyv_box_i64(v: i64) -> AnyValue;
    fn __torajs_anyv_box_pointer(p: *mut c_void) -> AnyValue;
}

/// `Some(answer)` for the empty-receiver mutator shape, `None` when
/// the proto singleton owns a `length` (caller keeps its loud
/// no-such exit) or the mid is not a mutator this table knows.
///
/// # Safety
/// `recv` is a BORROWED primitive-shaped AnyValue; `argv` holds
/// `argc` BORROWED NaN-box AnyValues.
pub(crate) unsafe fn prim_mut_method(
    proto_tag: i64,
    recv: AnyValue,
    mid: i64,
    argv: *const u64,
    argc: i64,
) -> Option<AnyValue> {
    unsafe {
        if proto_owns_length(proto_tag) {
            return None;
        }
        match mid {
            m if m == ANY_METHOD_POP || m == ANY_METHOD_SHIFT => Some(VALUE_UNDEFINED),
            m if m == ANY_METHOD_PUSH || m == ANY_METHOD_UNSHIFT => {
                Some(__torajs_anyv_box_i64(argc))
            }
            m if m == ANY_METHOD_REVERSE
                || m == ANY_METHOD_SORT
                || m == ANY_METHOD_FILL
                || m == ANY_METHOD_COPY_WITHIN =>
            {
                if m == ANY_METHOD_SORT && argc > 0 {
                    let cmp = *argv;
                    if !is_undefined(cmp) && crate::method_call::closure_boxed_entry(cmp).is_none()
                    {
                        return Some(crate::method_call::not_callable());
                    }
                }
                Some(crate::to_object::__torajs_any_to_object(recv))
            }
            m if m == ANY_METHOD_SPLICE => Some(__torajs_anyv_box_pointer(
                __torajs_arr_alloc_any(0) as *mut c_void,
            )),
            _ => None,
        }
    }
}

/// Does the family's proto singleton own a `length` (the G2d
/// expando face)? Mirrors the host pick in
/// [`crate::method_call_arraylike::arraylike_on_wrapper_proto`].
unsafe fn proto_owns_length(proto_tag: i64) -> bool {
    unsafe {
        let proto = torajs_rc::builtin_proto::__torajs_get_builtin_prototype(proto_tag);
        if proto.is_null() {
            return false;
        }
        let key = __torajs_str_alloc(b"length".as_ptr(), 6);
        let has = __torajs_dynobj_has(proto, key as *const c_void) != 0;
        __torajs_str_drop(key as *mut c_void);
        has
    }
}
