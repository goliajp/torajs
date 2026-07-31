//! `__torajs_reflect_apply` — §28.1.1 Reflect.apply(target, thisArg,
//! argumentsList) (rotation 267 刀 R6).
//!
//! The static shape of `Function.prototype.apply`: IsCallable gate on
//! the target, then the shared CreateListFromArrayLike + invoke
//! kernel ([`crate::method_call_closure::apply_list`]). One delta
//! from the prototype method: §28.1.1 step 2 runs
//! CreateListFromArrayLike unconditionally, so a nullish
//! argumentsList is a TypeError here — `f.apply(t)` / `f.apply(t,
//! null)`'s empty-list amnesty (§20.2.3.1 step 2) does not apply.

use core::ffi::c_char;

use crate::method_call_closure::{apply_list, call_target};
use crate::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr, is_cell, is_null, is_undefined};

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const c_char);
}

/// See module doc. Answers the call product; on a gate TypeError the
/// pending throw records and `undefined` rides back (the caller's
/// throw-check propagates before the value is consumed).
///
/// # Safety
/// Cell operands are valid heap pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_reflect_apply(
    f: AnyValue,
    this_arg: AnyValue,
    list: AnyValue,
) -> AnyValue {
    // §28.1.1 step 1 — IsCallable(target).
    let target = if is_cell(f) {
        unsafe { call_target(as_void_ptr(f)) }
    } else {
        None
    };
    let Some(target) = target else {
        unsafe {
            __torajs_throw_type_error(
                c"Reflect.apply requires the first argument be a function".as_ptr(),
            );
        }
        return VALUE_UNDEFINED;
    };
    // §28.1.1 step 2 — CreateListFromArrayLike with no nullish
    // amnesty (see module doc).
    if is_undefined(list) || is_null(list) {
        unsafe {
            __torajs_throw_type_error(
                c"Reflect.apply requires the third argument be an object".as_ptr(),
            );
        }
        return VALUE_UNDEFINED;
    }
    unsafe { apply_list(&target, this_arg, list) }
}
