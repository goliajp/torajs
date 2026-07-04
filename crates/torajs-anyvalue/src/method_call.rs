//! `__torajs_any_method_call` — `recv.name(args…)` where the
//! receiver is an `any` value (Any-method-call RFC 20260704 C1).
//!
//! ssa-lower interns the compile-time method name into an
//! `ANY_METHOD_*` id (torajs-rc), boxes each argument into a
//! stack-allocated argv of NaN-box AnyValues, and calls here; the
//! name bytes travel only for TypeError messages. Dispatch:
//!
//! - `null` / `undefined` receiver → catchable TypeError (ES
//!   §13.3.2 RequireObjectCoercible).
//! - ShortStr → materialize to a heap Str, reuse the Str arm, drop
//!   the temp.
//! - `Tag::Str` cell (Str or Substr view) → charAt /
//!   toUpperCase / toLowerCase glue (torajs-str `method_any`).
//! - `Tag::Arr` cell → push / pop glue (torajs-arr `method_any`;
//!   push writes the possibly-relocated receiver back through
//!   `recv_slot`).
//! - anything else (numeric immediates, other heap tags, unknown
//!   method ids) → catchable TypeError — the RFC's C2+ tags land
//!   here one arm at a time, never a silent wrong answer.
//!
//! Argument ledger: argv slots are BORROWED (the lowerer rc-decs
//! each one after the call); per-method glue incs what it keeps.
//! The returned AnyValue follows the boxed-value convention (cells
//! +1, owned by the caller).

use core::ffi::c_void;

use torajs_rc::{
    ANY_METHOD_CHAR_AT, ANY_METHOD_POP, ANY_METHOD_PUSH, ANY_METHOD_TO_LOWER_CASE,
    ANY_METHOD_TO_UPPER_CASE, Tag,
};

use crate::nanbox::{
    AnyValue, VALUE_UNDEFINED, as_void_ptr, is_cell, is_null, is_short_str, is_undefined,
};
use crate::nanbox_ffi_materialize::materialize_short_str;

unsafe extern "C" {
    /// torajs-arr — variadic push glue; returns the new length or
    /// the u64::MAX throw sentinel. Chases grow relocation and
    /// writes the fresh pointer back through `recv_slot` itself.
    fn __torajs_arr_any_push(
        arr: *mut c_void,
        argv: *const u64,
        argc: i64,
        recv_slot: *mut u64,
    ) -> u64;
    /// torajs-arr — pop glue (boxed last element, len shrink).
    fn __torajs_arr_any_pop(arr: *mut c_void) -> u64;
    /// torajs-str — charAt glue (empty string for OOB).
    fn __torajs_str_any_char_at(s: *mut u8, idx: i64) -> u64;
    /// torajs-str — toUpperCase / toLowerCase glue.
    fn __torajs_str_any_case(s: *const u8, upper: i64) -> u64;
    /// torajs-str — release a heap Str/Substr reference.
    fn __torajs_str_drop(s: *mut c_void);
    /// torajs-throw — record a pending catchable TypeError.
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

const ANY_METHOD_THREW: u64 = u64::MAX;

/// See module doc.
///
/// # Safety
/// Cell receivers are valid heap pointers; `argv` points at `argc`
/// AnyValue slots the caller keeps alive across the call;
/// `recv_slot` is NULL or the receiver variable's live slot.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_method_call(
    recv: AnyValue,
    mid: i64,
    _name: *const u8,
    _name_len: i64,
    recv_slot: *mut u64,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    if is_null(recv) || is_undefined(recv) {
        unsafe {
            __torajs_throw_type_error(c"cannot call a method of null or undefined".as_ptr());
        }
        return VALUE_UNDEFINED;
    }
    let arg_at = |i: i64| -> u64 {
        if i < argc {
            unsafe { *argv.add(i as usize) }
        } else {
            VALUE_UNDEFINED
        }
    };
    if is_short_str(recv) {
        // Materialize once, reuse the heap-Str arm, drop the temp
        // (results copy out of the temp's bytes, never alias it).
        unsafe {
            let tmp = materialize_short_str(recv);
            let out = str_method(tmp, mid, arg_at(0));
            __torajs_str_drop(tmp as *mut c_void);
            return out;
        }
    }
    if is_cell(recv) {
        let ptr = as_void_ptr(recv);
        let tag = unsafe { (ptr.cast::<u8>().add(4) as *const u16).read() };
        if tag == Tag::Str as u16 {
            return unsafe { str_method(ptr as *mut u8, mid, arg_at(0)) };
        }
        if tag == Tag::Arr as u16 {
            return unsafe { arr_method(ptr, mid, recv_slot, argv, argc) };
        }
    }
    unsafe {
        __torajs_throw_type_error(c"value is not a function on this any receiver".as_ptr());
    }
    VALUE_UNDEFINED
}

/// `Tag::Str` arm — id-switch onto the torajs-str glue.
unsafe fn str_method(s: *mut u8, mid: i64, arg0: u64) -> AnyValue {
    unsafe {
        match mid {
            m if m == ANY_METHOD_CHAR_AT => {
                let idx = crate::nanbox_ffi::__torajs_anyv_to_number(arg0);
                let idx = if idx.is_nan() { 0 } else { idx as i64 };
                __torajs_str_any_char_at(s, idx)
            }
            m if m == ANY_METHOD_TO_UPPER_CASE => __torajs_str_any_case(s, 1),
            m if m == ANY_METHOD_TO_LOWER_CASE => __torajs_str_any_case(s, 0),
            _ => method_not_a_function(),
        }
    }
}

/// `Tag::Arr` arm — id-switch onto the torajs-arr glue.
unsafe fn arr_method(
    arr: *mut c_void,
    mid: i64,
    recv_slot: *mut u64,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    unsafe {
        match mid {
            m if m == ANY_METHOD_PUSH => {
                let new_len = __torajs_arr_any_push(arr, argv, argc, recv_slot);
                if new_len == ANY_METHOD_THREW {
                    return VALUE_UNDEFINED;
                }
                crate::nanbox_encode::__torajs_anyv_box_i64(new_len as i64)
            }
            m if m == ANY_METHOD_POP => __torajs_arr_any_pop(arr),
            _ => method_not_a_function(),
        }
    }
}

unsafe fn method_not_a_function() -> AnyValue {
    unsafe {
        __torajs_throw_type_error(c"value is not a function on this any receiver".as_ptr());
    }
    VALUE_UNDEFINED
}
