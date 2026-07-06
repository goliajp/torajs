//! `Tag::WeakMap` / `Tag::WeakSet` arm of `__torajs_any_method_call`
//! (RFC 20260706-test262-bug-corpus RC-2b) — split out of
//! `method_call.rs` by the 500-line file discipline, mirroring the
//! `method_call_mapset` sibling.
//!
//! Routes onto the torajs-weak kernels (ptr-keyed, values held with
//! rc semantics):
//!
//! - WeakMap: get / set / has / delete. WeakSet: add / has / delete.
//!   Methods of the other kind fall to the catchable TypeError.
//! - Keys must be heap cells ("held weakly", ES §24.3/§24.4): a
//!   primitive key reads as absent (`has` → false, `delete` → false,
//!   `get` → undefined) and THROWS for `set` / `add` (spec
//!   "Invalid value used as weak map key").
//! - `set` values ride the kernel's heap-ptr rc lane, so only cell
//!   values pass; a primitive value is a loud TypeError until the
//!   kernel's value slot widens to a boxed AnyValue (RFC records the
//!   follow-up lane).
//! - `set` / `add` return `this` (+1, boxed-value convention);
//!   `get` boxes the kernel's already-inc'd hit or `undefined`.

use core::ffi::c_void;

use torajs_rc::{
    __torajs_rc_inc, ANY_METHOD_ADD, ANY_METHOD_DELETE, ANY_METHOD_GET, ANY_METHOD_HAS,
    ANY_METHOD_SET,
};

use crate::method_call::method_not_a_function;
use crate::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr, is_cell};
use crate::nanbox_encode::{__torajs_anyv_box_from_pair, __torajs_anyv_box_pointer};

unsafe extern "C" {
    /// torajs-weak — ptr-keyed kernels. Keys are held weakly (no
    /// rc); `set` incs the value it keeps, `get` incs the hit it
    /// hands out.
    fn __torajs_weakmap_set(p: *mut c_void, key: *mut c_void, value: *mut c_void);
    fn __torajs_weakmap_get(p: *mut c_void, key: *mut c_void) -> *mut c_void;
    fn __torajs_weakmap_has(p: *mut c_void, key: *mut c_void) -> i64;
    fn __torajs_weakmap_delete(p: *mut c_void, key: *mut c_void) -> i64;
    fn __torajs_weakset_add(p: *mut c_void, key: *mut c_void);
    fn __torajs_weakset_has(p: *mut c_void, key: *mut c_void) -> i64;
    fn __torajs_weakset_delete(p: *mut c_void, key: *mut c_void) -> i64;
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

/// Dispatch a method id on a `Tag::WeakMap` / `Tag::WeakSet` cell.
///
/// # Safety
/// `ptr` is a live WeakMap/WeakSet cell; `argv` points at `argc`
/// AnyValue slots the caller keeps alive across the call (BORROWED).
pub(crate) unsafe fn weak_method(
    ptr: *mut c_void,
    is_weakset: bool,
    mid: i64,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    let arg_at = |i: i64| -> u64 {
        if i < argc {
            unsafe { *argv.add(i as usize) }
        } else {
            VALUE_UNDEFINED
        }
    };
    let key_av = arg_at(0);
    let key: *mut c_void = if is_cell(key_av) {
        as_void_ptr(key_av)
    } else {
        core::ptr::null_mut()
    };
    unsafe {
        match mid {
            m if m == ANY_METHOD_HAS => {
                let hit = if key.is_null() {
                    0
                } else if is_weakset {
                    __torajs_weakset_has(ptr, key)
                } else {
                    __torajs_weakmap_has(ptr, key)
                };
                __torajs_anyv_box_from_pair(1, hit)
            }
            m if m == ANY_METHOD_DELETE => {
                let hit = if key.is_null() {
                    0
                } else if is_weakset {
                    __torajs_weakset_delete(ptr, key)
                } else {
                    __torajs_weakmap_delete(ptr, key)
                };
                __torajs_anyv_box_from_pair(1, hit)
            }
            m if m == ANY_METHOD_GET && !is_weakset => {
                if key.is_null() {
                    return VALUE_UNDEFINED;
                }
                let v = __torajs_weakmap_get(ptr, key);
                if v.is_null() {
                    VALUE_UNDEFINED
                } else {
                    // Kernel already handed out a +1.
                    __torajs_anyv_box_pointer(v)
                }
            }
            m if (m == ANY_METHOD_SET && !is_weakset) || (m == ANY_METHOD_ADD && is_weakset) => {
                if key.is_null() {
                    __torajs_throw_type_error(c"Invalid value used as weak map key".as_ptr());
                    return VALUE_UNDEFINED;
                }
                if is_weakset {
                    __torajs_weakset_add(ptr, key);
                } else {
                    let val_av = arg_at(1);
                    if !is_cell(val_av) {
                        // Kernel value slot is a heap-ptr rc lane —
                        // the boxed-AnyValue value lane is the RFC's
                        // recorded follow-up.
                        __torajs_throw_type_error(
                            c"WeakMap.set with a non-object value is not yet supported on an any receiver"
                                .as_ptr(),
                        );
                        return VALUE_UNDEFINED;
                    }
                    __torajs_weakmap_set(ptr, key, as_void_ptr(val_av));
                }
                // set / add return `this` (+1, boxed-value convention).
                __torajs_rc_inc(ptr);
                __torajs_anyv_box_from_pair(4, ptr as i64)
            }
            _ => method_not_a_function(),
        }
    }
}
