//! `Tag::Arr` receiver arm for `__torajs_any_method_call` (split out
//! of `method_call.rs` at the 500-line boundary, chunk 710 — the
//! dispatcher stays the tag-switch, the torajs-arr glue id-switch
//! lives here; same shape as the str / num / date siblings).
//!
//! Argument ledger: identical to the dispatcher — argv slots are
//! BORROWED; growth-relocating methods (push / unshift) write the
//! possibly-moved receiver back through `recv_slot` themselves.

use core::ffi::c_void;

use torajs_rc::{
    ANY_METHOD_FILTER, ANY_METHOD_FOR_EACH, ANY_METHOD_INCLUDES, ANY_METHOD_INDEX_OF,
    ANY_METHOD_JOIN, ANY_METHOD_LAST_INDEX_OF, ANY_METHOD_MAP, ANY_METHOD_POP, ANY_METHOD_PUSH,
    ANY_METHOD_REVERSE, ANY_METHOD_SHIFT, ANY_METHOD_SLICE, ANY_METHOD_UNSHIFT,
};

use crate::method_call::{closure_boxed_entry, method_no_such, not_callable, to_index};
use crate::nanbox::{AnyValue, VALUE_UNDEFINED, is_undefined};
use crate::nanbox_encode::{
    __torajs_anyv_box_from_pair, __torajs_anyv_box_i64, __torajs_anyv_box_pointer,
};
use crate::nanbox_ffi::__torajs_anyv_to_str;

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
    /// torajs-arr — shift glue (boxed first element, forward move).
    fn __torajs_arr_any_shift(arr: *mut c_void) -> u64;
    /// torajs-arr — variadic unshift glue; new length or the
    /// u64::MAX throw sentinel; relocations via `recv_slot`.
    fn __torajs_arr_any_unshift(
        arr: *mut c_void,
        argv: *const u64,
        argc: i64,
        recv_slot: *mut u64,
    ) -> u64;
    /// torajs-arr — strict-eq index scan (found index or -1).
    fn __torajs_arr_any_index_of(arr: *const c_void, needle: u64, from: i64) -> i64;
    /// torajs-arr — backwards strict-eq scan (§23.1.3.20).
    fn __torajs_arr_any_last_index_of(arr: *const c_void, needle: u64, from: i64) -> i64;
    /// torajs-arr — in-place 8-byte-slot swap (element-type-agnostic
    /// — FLAG_ARR_ANY slots are 8-byte NaN-box immediates too);
    /// answers the same pointer for chaining.
    fn __torajs_arr_reverse(arr: *mut u8) -> *mut u8;
    /// torajs-arr — SameValueZero scan (1/0).
    fn __torajs_arr_any_includes(arr: *const c_void, needle: u64, from: i64) -> i64;
    /// torajs-arr — element-kind-dispatched join (fresh Str).
    fn __torajs_arr_any_join(arr: *const u8, sep: *const u8) -> u64;
    /// torajs-arr — kind-aware slice for any receivers (C4+); the
    /// returned array is fresh +1 rc, same slot layout as the source.
    fn __torajs_arr_any_slice(arr: *const u8, start: i64, end: i64) -> *mut u8;
    /// torajs-arr — HO loops over the boxed dual-entry callback ABI.
    fn __torajs_arr_any_map(arr: *const c_void, cb_env: *mut c_void, cb_entry: u64) -> u64;
    fn __torajs_arr_any_filter(arr: *const c_void, cb_env: *mut c_void, cb_entry: u64) -> u64;
    fn __torajs_arr_any_for_each(arr: *const c_void, cb_env: *mut c_void, cb_entry: u64) -> u64;
    /// torajs-str — allocate a fresh Str from raw bytes (the
    /// `join()` default "," separator).
    fn __torajs_str_alloc(src: *const u8, len: i64) -> *mut u8;
    /// torajs-str — release a heap Str/Substr reference.
    fn __torajs_str_drop(s: *mut c_void);
}

/// Throw sentinel the torajs-arr glue floats — mirror of
/// `method_call.rs::ANY_METHOD_THREW`.
const ANY_METHOD_THREW: u64 = u64::MAX;

/// `Tag::Arr` arm — id-switch onto the torajs-arr glue.
pub(crate) unsafe fn arr_method(
    arr: *mut c_void,
    mid: i64,
    recv_slot: *mut u64,
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
    unsafe {
        match mid {
            m if m == ANY_METHOD_PUSH => {
                let new_len = __torajs_arr_any_push(arr, argv, argc, recv_slot);
                if new_len == ANY_METHOD_THREW {
                    return VALUE_UNDEFINED;
                }
                __torajs_anyv_box_i64(new_len as i64)
            }
            m if m == ANY_METHOD_POP => __torajs_arr_any_pop(arr),
            m if m == ANY_METHOD_SHIFT => __torajs_arr_any_shift(arr),
            m if m == ANY_METHOD_UNSHIFT => {
                let new_len = __torajs_arr_any_unshift(arr, argv, argc, recv_slot);
                if new_len == ANY_METHOD_THREW {
                    return VALUE_UNDEFINED;
                }
                __torajs_anyv_box_i64(new_len as i64)
            }
            m if m == ANY_METHOD_INDEX_OF => {
                let from = to_index(arg_at(1), 0);
                __torajs_anyv_box_i64(__torajs_arr_any_index_of(arr, arg_at(0), from))
            }
            m if m == ANY_METHOD_LAST_INDEX_OF => {
                // §23.1.3.20 step 4: absent fromIndex starts at the
                // last slot (i64::MAX rides the kernel clamp); a
                // PRESENT undefined is ToIntegerOrInfinity(undefined)
                // = 0 — only slot 0 is scanned.
                let from = if argc >= 2 {
                    to_index(arg_at(1), 0)
                } else {
                    i64::MAX
                };
                __torajs_anyv_box_i64(__torajs_arr_any_last_index_of(arr, arg_at(0), from))
            }
            m if m == ANY_METHOD_REVERSE => {
                let p = __torajs_arr_reverse(arr as *mut u8);
                // The receiver is the return value (chaining) — the
                // owned return protocol takes a fresh stake.
                torajs_rc::__torajs_rc_inc(p as *mut c_void);
                __torajs_anyv_box_pointer(p as *mut c_void)
            }
            m if m == ANY_METHOD_INCLUDES => {
                let from = to_index(arg_at(1), 0);
                __torajs_anyv_box_from_pair(1, __torajs_arr_any_includes(arr, arg_at(0), from))
            }
            m if m == ANY_METHOD_MAP || m == ANY_METHOD_FILTER || m == ANY_METHOD_FOR_EACH => {
                let Some((cb_env, cb_entry)) = closure_boxed_entry(arg_at(0)) else {
                    return not_callable();
                };
                if m == ANY_METHOD_MAP {
                    __torajs_arr_any_map(arr, cb_env, cb_entry)
                } else if m == ANY_METHOD_FILTER {
                    __torajs_arr_any_filter(arr, cb_env, cb_entry)
                } else {
                    __torajs_arr_any_for_each(arr, cb_env, cb_entry)
                }
            }
            m if m == ANY_METHOD_JOIN => {
                // ES §23.1.3.18 step 2: missing sep means ",".
                let sep_av = arg_at(0);
                let sep = if is_undefined(sep_av) {
                    __torajs_str_alloc(c",".as_ptr() as *const u8, 1)
                } else {
                    __torajs_anyv_to_str(sep_av) as *mut u8
                };
                let out = __torajs_arr_any_join(arr as *const u8, sep as *const u8);
                __torajs_str_drop(sep as *mut c_void);
                out
            }
            m if m == ANY_METHOD_SLICE => {
                // Missing end rides the kernel clamp (same idiom as
                // the str arm's slice).
                let start = to_index(arg_at(0), 0);
                let end = to_index(arg_at(1), i64::MAX);
                let p = __torajs_arr_any_slice(arr as *const u8, start, end);
                __torajs_anyv_box_pointer(p as *mut c_void)
            }
            _ => method_no_such(),
        }
    }
}
