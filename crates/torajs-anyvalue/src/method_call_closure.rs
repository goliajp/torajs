//! `Tag::Closure` receiver arm for `__torajs_any_method_call`
//! (chunk 710) — `Function.prototype.call` / `apply` on closure
//! values that travel through the `any` world.
//!
//! A torajs closure body cannot reference `this` (class methods
//! dispatch through the vtable tier, never as bare closure cells),
//! so the ES thisArg has no observable binding to project — both
//! methods drop it and invoke the boxed dual entry with the
//! remaining arguments:
//!
//! - `f.call(thisArg, a, b)` → `invoke_boxed(env, entry, argv[1..])`.
//! - `f.apply(thisArg, list)` → the list unpacks per
//!   CreateListFromArrayLike (ES §7.3.19): `undefined` / `null` is
//!   an empty list; an `Arr` cell reads element-by-element through
//!   the kind-aware `__torajs_arr_index_get` (owned boxes, released
//!   after the call); anything else is a catchable TypeError.
//! - An expando property (chunk 529's lazy props bag) shadows the
//!   builtin per ES own-property order — `f.call = …` wins — and
//!   dispatches through the dynobj arm.
//! - A closure without a boxed dual entry cannot dispatch
//!   dynamically ([`not_callable`], same as the bare any-call lane).
//! - Every other method id floats the no-such sentinel (`toString`
//!   source text and `bind` are recorded boundaries).
//!
//! Argument ledger: identical to the dispatcher — argv slots are
//! BORROWED; the apply unpacking's element boxes are this arm's own
//! temps and drop before returning.

use core::ffi::c_void;

use torajs_rc::{ANY_METHOD_APPLY, ANY_METHOD_CALL, Tag};

use crate::method_call::{
    MAX_BOXED_ARGS, closure_cell_entry, invoke_boxed, method_no_such, not_callable,
};
use crate::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr, is_cell, is_null, is_undefined};

unsafe extern "C" {
    /// torajs-dynobj — own-property probe (5 = absent).
    fn __torajs_dynobj_get_tag(obj: *const c_void, key: *const c_void) -> u64;
    /// torajs-arr — kind-aware boxed element read (owned +1 for
    /// cells; holes and OOB answer undefined).
    fn __torajs_arr_index_get(arr: *const c_void, idx: i64) -> u64;
    /// torajs-throw — record a pending catchable TypeError.
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

/// Closure-cell lazy props slot — mirror of torajs-core
/// `ssa_lower.rs::CLOSURE_PROPS_OFF`.
const CLOSURE_PROPS_OFF: usize = 24;

/// Arr cell length slot — mirror of torajs-arr `layout::ARR_LEN_OFF`.
const ARR_LEN_OFF: usize = 8;

/// `Tag::Closure` arm — see module doc.
///
/// # Safety
/// `ptr` is a valid `Tag::Closure` heap pointer; `argv` points at
/// `argc` AnyValue slots the caller keeps alive across the call;
/// `name_str` is NULL or a live Str cell.
pub(crate) unsafe fn closure_method(
    ptr: *mut c_void,
    mid: i64,
    name_str: *const u8,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    unsafe {
        // ES own-property order: an expando shadows the builtin.
        let props = *(ptr.cast::<u8>().add(CLOSURE_PROPS_OFF) as *const u64) as *const c_void;
        if !props.is_null()
            && !name_str.is_null()
            && __torajs_dynobj_get_tag(props, name_str as *const c_void) != 5
        {
            return crate::method_call_dynobj::dynobj_method(
                props as *mut c_void,
                name_str,
                argv,
                argc,
            );
        }
        match mid {
            m if m == ANY_METHOD_CALL => {
                let Some((env, entry)) = closure_cell_entry(ptr) else {
                    return not_callable();
                };
                if argc <= 1 {
                    return invoke_boxed(env, entry, argv, 0);
                }
                invoke_boxed(env, entry, argv.add(1), argc - 1)
            }
            m if m == ANY_METHOD_APPLY => {
                let Some((env, entry)) = closure_cell_entry(ptr) else {
                    return not_callable();
                };
                let list = if argc >= 2 {
                    *argv.add(1)
                } else {
                    VALUE_UNDEFINED
                };
                apply_list(env, entry, list)
            }
            _ => method_no_such(),
        }
    }
}

/// CreateListFromArrayLike + invoke — `undefined` / `null` is an
/// empty list, an `Arr` cell unpacks element-by-element, everything
/// else is a catchable TypeError.
unsafe fn apply_list(env: *mut c_void, entry: u64, list: AnyValue) -> AnyValue {
    unsafe {
        if is_undefined(list) || is_null(list) {
            return invoke_boxed(env, entry, core::ptr::null(), 0);
        }
        let arr = if is_cell(list) {
            as_void_ptr(list)
        } else {
            core::ptr::null_mut()
        };
        if arr.is_null() || (arr.cast::<u8>().add(4) as *const u16).read() != Tag::Arr as u16 {
            __torajs_throw_type_error(
                c"second argument to Function.prototype.apply must be an array".as_ptr(),
            );
            return VALUE_UNDEFINED;
        }
        let n = *(arr.cast::<u8>().add(ARR_LEN_OFF) as *const u64) as usize;
        // Element boxes are owned temps (+1 per cell) — released
        // after the invoke; the common small shape stays on the
        // stack, mirror of `invoke_boxed`'s own buffer split.
        let out;
        if n > MAX_BOXED_ARGS {
            let boxed: Vec<u64> = (0..n)
                .map(|i| __torajs_arr_index_get(arr, i as i64))
                .collect();
            out = invoke_boxed(env, entry, boxed.as_ptr(), n as i64);
            for b in boxed {
                crate::nanbox_ffi::__torajs_anyv_rc_dec(b);
            }
        } else {
            let mut buf = [VALUE_UNDEFINED; MAX_BOXED_ARGS];
            for (i, slot) in buf.iter_mut().enumerate().take(n) {
                *slot = __torajs_arr_index_get(arr, i as i64);
            }
            out = invoke_boxed(env, entry, buf.as_ptr(), n as i64);
            for b in buf.iter().take(n) {
                crate::nanbox_ffi::__torajs_anyv_rc_dec(*b);
            }
        }
        out
    }
}
