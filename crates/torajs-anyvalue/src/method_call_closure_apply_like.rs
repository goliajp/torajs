//! §7.3.18 CreateListFromArrayLike over a NON-Arr object — the
//! `f.apply(t, argArray)` / `Reflect.apply` tail the dense-Arr fast
//! lane in [`crate::method_call_closure::apply_list`] refused with
//! "must be an array" (r292: the 15.3.4.3-*-s family hands a
//! FUNCTION as argArray; the spec admits any object and reads it
//! array-like). Split beside the closure kernel at the size cap.
//!
//! The reads ride [`crate::iter_any_result::iter_result_get`] — the
//! real [[Get]] (struct-field fast lane, accessor-aware dispatcher
//! lane) — so a poisoned `length` / index getter forwards its throw,
//! and a closure argArray answers its virtual `length` face.

use core::ffi::c_void;

use torajs_rc::Tag;

use crate::iter_any_result::iter_result_get;
use crate::method_call::MAX_BOXED_ARGS;
use crate::method_call_closure::{CallTarget, dispatch};
use crate::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr, is_cell, is_null, is_undefined};
use crate::nanbox_ffi::__torajs_anyv_rc_dec;

unsafe extern "C" {
    fn __torajs_throw_check() -> i64;
    /// torajs-throw — record a pending catchable TypeError.
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    /// torajs-arr — kind-aware boxed element read (owned +1 for
    /// cells; holes and OOB answer undefined).
    fn __torajs_arr_index_get(arr: *const c_void, idx: i64) -> u64;
}

/// Arr cell length slot — mirror of torajs-arr `layout::ARR_LEN_OFF`.
const ARR_LEN_OFF: usize = 8;

/// §7.3.18 steps 3-8: `ToLength(Get(obj, "length"))`, then per-index
/// `Get(obj, ToString(i))`, then invoke. Abrupt completions forward
/// with every collected element released.
///
/// # Safety
/// `list` is a live cell whose pointer is `list_ptr`; `target` came
/// from `call_target`.
pub(crate) unsafe fn apply_array_like(
    target: &CallTarget,
    this_arg: AnyValue,
    list: AnyValue,
    list_ptr: *mut c_void,
) -> AnyValue {
    unsafe {
        let Some(len_v) = iter_result_get(list, list_ptr, b"length") else {
            return VALUE_UNDEFINED;
        };
        let (t, p) = (
            crate::__torajs_anyv_unbox_tag(len_v),
            crate::__torajs_anyv_unbox_value(len_v),
        );
        let n_f = crate::coerce::any_to_number(t, p);
        __torajs_anyv_rc_dec(len_v);
        if __torajs_throw_check() != 0 {
            return VALUE_UNDEFINED;
        }
        // §7.1.20 ToLength — NaN / negative → 0, truncate, clamp to
        // 2^53 - 1.
        let n = if n_f.is_nan() || n_f <= 0.0 {
            0
        } else {
            n_f.trunc().min(9007199254740991.0) as usize
        };
        let mut boxed: Vec<u64> = Vec::with_capacity(n);
        for i in 0..n {
            let key = i.to_string();
            let Some(v) = iter_result_get(list, list_ptr, key.as_bytes()) else {
                for b in &boxed {
                    __torajs_anyv_rc_dec(*b);
                }
                return VALUE_UNDEFINED;
            };
            boxed.push(v);
        }
        let out = dispatch(target, this_arg, boxed.as_ptr(), n as i64);
        for b in boxed {
            __torajs_anyv_rc_dec(b);
        }
        out
    }
}

/// CreateListFromArrayLike + invoke — `undefined` / `null` is an
/// empty list, an `Arr` cell unpacks element-by-element on the fast
/// lane, any other object reads array-like above, and a primitive
/// is a catchable TypeError.
pub(crate) unsafe fn apply_list(
    target: &CallTarget,
    this_arg: AnyValue,
    list: AnyValue,
) -> AnyValue {
    unsafe {
        if is_undefined(list) || is_null(list) {
            return dispatch(target, this_arg, core::ptr::null(), 0);
        }
        let arr = if is_cell(list) {
            as_void_ptr(list)
        } else {
            core::ptr::null_mut()
        };
        let arr_tag = if arr.is_null() {
            0
        } else {
            (arr.cast::<u8>().add(4) as *const u16).read()
        };
        if arr.is_null()
            || arr_tag == Tag::Str as u16
            || arr_tag == Tag::Symbol as u16
            || arr_tag == Tag::BigInt as u16
        {
            // §7.3.18 step 2 — a primitive argArray (number, bool,
            // string, symbol, bigint — the heap-celled primitives
            // included) is a TypeError. (r292 sweep caught the
            // Symbol cell riding the array-like lane —
            // argarray-not-object regression.)
            __torajs_throw_type_error(
                c"second argument to Function.prototype.apply must be an array".as_ptr(),
            );
            return VALUE_UNDEFINED;
        }
        if arr_tag != Tag::Arr as u16 {
            // Any other OBJECT reads array-like (r292 — §7.3.18
            // CreateListFromArrayLike; the dense-Arr lane below
            // stays the fast path).
            return apply_array_like(target, this_arg, list, arr);
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
            out = dispatch(target, this_arg, boxed.as_ptr(), n as i64);
            for b in boxed {
                __torajs_anyv_rc_dec(b);
            }
        } else {
            let mut buf = [VALUE_UNDEFINED; MAX_BOXED_ARGS];
            for (i, slot) in buf.iter_mut().enumerate().take(n) {
                *slot = __torajs_arr_index_get(arr, i as i64);
            }
            out = dispatch(target, this_arg, buf.as_ptr(), n as i64);
            for b in buf.iter().take(n) {
                __torajs_anyv_rc_dec(*b);
            }
        }
        out
    }
}
