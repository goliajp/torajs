//! Callback half of the generic array-like arm (RFC 20260712
//! chunks 2+3a) — every / some / forEach / map / filter / the find
//! family / reduce family over a `Tag::DynObj` receiver. Sibling of
//! `method_call_arraylike.rs` (which owns the length / Get / Has
//! helpers and the scan family).
//!
//! Spec order the tests observe: the receiver's length getter has
//! ALREADY run (the caller reads it before dispatching here); a
//! non-callable callback is the TypeError AFTER that (§23.1.3.6
//! step 3). HasProperty gates the every/some/forEach/map/filter and
//! reduce walks (holes skip); the find family Gets every index.

use core::ffi::c_void;

use torajs_rc::{
    ANY_METHOD_EVERY, ANY_METHOD_FILTER, ANY_METHOD_FIND, ANY_METHOD_FIND_INDEX,
    ANY_METHOD_FIND_LAST, ANY_METHOD_FIND_LAST_INDEX, ANY_METHOD_FLAT_MAP, ANY_METHOD_MAP,
    ANY_METHOD_REDUCE, ANY_METHOD_REDUCE_RIGHT, ANY_METHOD_SOME, Tag,
};

use crate::method_call::{
    MAX_BOXED_ARGS, closure_boxed_entry, invoke_boxed, not_callable, recv_first_shift,
};
use crate::method_call_arraylike::{arraylike_get, arraylike_has};
use crate::nanbox::{AnyValue, VALUE_FALSE, VALUE_TRUE, VALUE_UNDEFINED};
use crate::nanbox_encode::{
    __torajs_anyv_box_i64, __torajs_anyv_box_pointer, __torajs_anyv_unbox_tag,
    __torajs_anyv_unbox_value,
};
use crate::nanbox_ffi::__torajs_anyv_to_bool;

unsafe extern "C" {
    /// torajs-throw — pending-throw flag (1 = a throw is recorded).
    fn __torajs_throw_check() -> i64;
    /// torajs-throw — map's §23.1.3.20 step 4 ArraySpeciesCreate cap
    /// (records the pending throw and returns).
    fn __torajs_throw_range_error(msg: *const core::ffi::c_char);
    /// torajs-arr — fresh Array<Any> (map / filter products).
    fn __torajs_arr_alloc_any(cap: u64) -> *mut u8;
    /// torajs-arr — append one (tag, value) pair; ANY_HEAP transfers
    /// ownership. Caller must capture the (possibly moved) return.
    fn __torajs_arr_push_any(arr: *mut c_void, tag: u64, value: u64) -> *mut u8;
    /// torajs-arr — the §23.1.3.24 empty-no-init TypeErrors (message
    /// parity with the Arr arm).
    fn __torajs_arr_throw_reduce_empty();
    fn __torajs_arr_throw_reduce_right_empty();
    /// torajs-arr — splice src's slots onto dst (copy + inc; src
    /// keeps its own stake). Caller captures the moved return.
    fn __torajs_arr_extend_any(dst: *mut u8, src: *const u8) -> *mut u8;
    /// Cross-tier — universal NaN-box-safe heap-value release.
    fn __torajs_value_drop_heap(p: *mut c_void);
}

/// Invoke the callback with `(v, k, O)` — `v`'s stake stays with
/// the caller; the return is owned. A recv-first callback (RFC
/// 20260717-objlit-anylane-recv knife 2e) shifts the triple up one
/// slot so `__this` reads the buffer's `undefined` (no-thisArg HOF,
/// `this = undefined`).
unsafe fn call_cb(
    cb_env: *mut c_void,
    cb_entry: u64,
    v: AnyValue,
    k: i64,
    obj_boxed: AnyValue,
) -> AnyValue {
    unsafe {
        let s = recv_first_shift(cb_env);
        let mut argv = [VALUE_UNDEFINED; MAX_BOXED_ARGS];
        argv[s] = v;
        argv[s + 1] = __torajs_anyv_box_i64(k);
        argv[s + 2] = obj_boxed;
        invoke_boxed(cb_env, cb_entry, argv.as_ptr(), (3 + s) as i64)
    }
}

/// §23.1.3.11 step 8.c-d — an array-valued mapped element spreads
/// exactly one level (its slots copy + inc through extend; the
/// mapped temp's own stake drops here); everything else appends
/// as-is (its stake transfers into the slot). Answers the
/// possibly-moved product.
unsafe fn flat_map_append(product: *mut u8, r: AnyValue) -> *mut u8 {
    unsafe {
        let is_arr = crate::nanbox::is_cell(r) && {
            let p = crate::nanbox::as_void_ptr(r);
            (p.cast::<u8>().add(4) as *const u16).read() == Tag::Arr as u16
        };
        if is_arr {
            let out = __torajs_arr_extend_any(product, crate::nanbox::as_void_ptr(r) as *const u8);
            __torajs_value_drop_heap(r as *mut c_void);
            out
        } else {
            __torajs_arr_push_any(
                product as *mut c_void,
                __torajs_anyv_unbox_tag(r) as u64,
                __torajs_anyv_unbox_value(r) as u64,
            )
        }
    }
}

/// The find family (§23.1.3.8-.10) — Gets EVERY index (no Has
/// gate), ascending or descending per the mid; the index flavors
/// answer k / -1, the value flavors the element / undefined.
unsafe fn arraylike_find(
    obj: *mut c_void,
    mid: i64,
    len: i64,
    cb_env: *mut c_void,
    cb_entry: u64,
    obj_boxed: AnyValue,
) -> AnyValue {
    unsafe {
        let right = mid == ANY_METHOD_FIND_LAST || mid == ANY_METHOD_FIND_LAST_INDEX;
        let wants_index = mid == ANY_METHOD_FIND_INDEX || mid == ANY_METHOD_FIND_LAST_INDEX;
        let mut step: i64 = 0;
        while step < len {
            let k = if right { len - 1 - step } else { step };
            step += 1;
            let v = arraylike_get(obj, k);
            let r = call_cb(cb_env, cb_entry, v, k, obj_boxed);
            if __torajs_throw_check() != 0 {
                __torajs_value_drop_heap(v as *mut c_void);
                __torajs_value_drop_heap(r as *mut c_void);
                return VALUE_UNDEFINED;
            }
            let hit = __torajs_anyv_to_bool(r);
            __torajs_value_drop_heap(r as *mut c_void);
            if hit {
                if wants_index {
                    __torajs_value_drop_heap(v as *mut c_void);
                    return __torajs_anyv_box_i64(k);
                }
                return v;
            }
            __torajs_value_drop_heap(v as *mut c_void);
        }
        if wants_index {
            __torajs_anyv_box_i64(-1)
        } else {
            VALUE_UNDEFINED
        }
    }
}

/// See module doc — `len` arrives from the caller's already-run
/// length read.
pub(crate) unsafe fn arraylike_hof(
    obj: *mut c_void,
    mid: i64,
    len: i64,
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
        let Some((cb_env, cb_entry)) = closure_boxed_entry(arg_at(0)) else {
            return not_callable();
        };
        let obj_boxed = __torajs_anyv_box_pointer(obj);
        match mid {
            m if m == ANY_METHOD_REDUCE || m == ANY_METHOD_REDUCE_RIGHT => {
                let right = m == ANY_METHOD_REDUCE_RIGHT;
                let has_init = argc >= 2;
                let mut acc = if has_init { arg_at(1) } else { VALUE_UNDEFINED };
                if has_init {
                    // The borrowed init becomes the owned accumulator.
                    torajs_rc::__torajs_rc_inc(acc as *mut c_void);
                }
                let mut seeded = has_init;
                let mut step: i64 = 0;
                while step < len {
                    let k = if right { len - 1 - step } else { step };
                    step += 1;
                    if !arraylike_has(obj, k) {
                        continue;
                    }
                    let v = arraylike_get(obj, k);
                    if !seeded {
                        acc = v;
                        seeded = true;
                        continue;
                    }
                    // Recv-first shift — see `call_cb`.
                    let s = recv_first_shift(cb_env);
                    let mut cb_argv = [VALUE_UNDEFINED; MAX_BOXED_ARGS];
                    cb_argv[s] = acc;
                    cb_argv[s + 1] = v;
                    cb_argv[s + 2] = __torajs_anyv_box_i64(k);
                    cb_argv[s + 3] = obj_boxed;
                    let r = invoke_boxed(cb_env, cb_entry, cb_argv.as_ptr(), (4 + s) as i64);
                    __torajs_value_drop_heap(acc as *mut c_void);
                    __torajs_value_drop_heap(v as *mut c_void);
                    if __torajs_throw_check() != 0 {
                        __torajs_value_drop_heap(r as *mut c_void);
                        return VALUE_UNDEFINED;
                    }
                    acc = r;
                }
                if !seeded {
                    if right {
                        __torajs_arr_throw_reduce_right_empty();
                    } else {
                        __torajs_arr_throw_reduce_empty();
                    }
                    return VALUE_UNDEFINED;
                }
                acc
            }
            m if m == ANY_METHOD_FIND
                || m == ANY_METHOD_FIND_INDEX
                || m == ANY_METHOD_FIND_LAST
                || m == ANY_METHOD_FIND_LAST_INDEX =>
            {
                arraylike_find(obj, m, len, cb_env, cb_entry, obj_boxed)
            }
            // every / some / forEach / map / filter — has-gated walk.
            _ => {
                // §23.1.3.20 step 4 — map's ArraySpeciesCreate(O, len)
                // throws RangeError above 2^32-1 before any element
                // read (O(1) early exit; the clamp walked ~2^53
                // indexes, which reads as a hang). The other walkers
                // stay ungated: every/some/forEach create no product
                // and filter/flatMap seed ArraySpeciesCreate(O, 0).
                if mid == ANY_METHOD_MAP && len > 4294967295 {
                    __torajs_throw_range_error(
                        c"Array length must be a positive integer of safe magnitude.".as_ptr(),
                    );
                    return VALUE_UNDEFINED;
                }
                // Cap hint only (push grows on demand) — a
                // ToLength-sized length must not size the malloc.
                let mut product: *mut u8 = if mid == ANY_METHOD_MAP
                    || mid == ANY_METHOD_FILTER
                    || mid == ANY_METHOD_FLAT_MAP
                {
                    __torajs_arr_alloc_any(len.clamp(0, 4096) as u64)
                } else {
                    core::ptr::null_mut()
                };
                let mut k: i64 = 0;
                while k < len {
                    if !arraylike_has(obj, k) {
                        // Dense-emulation boundary: map keeps result
                        // indexes aligned by filling undefined.
                        if mid == ANY_METHOD_MAP {
                            product = __torajs_arr_push_any(product as *mut c_void, 0, 0);
                        }
                        k += 1;
                        continue;
                    }
                    let v = arraylike_get(obj, k);
                    let r = call_cb(cb_env, cb_entry, v, k, obj_boxed);
                    if __torajs_throw_check() != 0 {
                        __torajs_value_drop_heap(v as *mut c_void);
                        __torajs_value_drop_heap(r as *mut c_void);
                        if !product.is_null() {
                            __torajs_value_drop_heap(product as *mut c_void);
                        }
                        return VALUE_UNDEFINED;
                    }
                    match mid {
                        mm if mm == ANY_METHOD_EVERY => {
                            let ok = __torajs_anyv_to_bool(r);
                            __torajs_value_drop_heap(r as *mut c_void);
                            __torajs_value_drop_heap(v as *mut c_void);
                            if !ok {
                                return VALUE_FALSE;
                            }
                        }
                        mm if mm == ANY_METHOD_SOME => {
                            let ok = __torajs_anyv_to_bool(r);
                            __torajs_value_drop_heap(r as *mut c_void);
                            __torajs_value_drop_heap(v as *mut c_void);
                            if ok {
                                return VALUE_TRUE;
                            }
                        }
                        mm if mm == ANY_METHOD_MAP => {
                            // r's stake transfers into the slot.
                            product = __torajs_arr_push_any(
                                product as *mut c_void,
                                __torajs_anyv_unbox_tag(r) as u64,
                                __torajs_anyv_unbox_value(r) as u64,
                            );
                            __torajs_value_drop_heap(v as *mut c_void);
                        }
                        mm if mm == ANY_METHOD_FLAT_MAP => {
                            product = flat_map_append(product, r);
                            __torajs_value_drop_heap(v as *mut c_void);
                        }
                        mm if mm == ANY_METHOD_FILTER => {
                            let keep = __torajs_anyv_to_bool(r);
                            __torajs_value_drop_heap(r as *mut c_void);
                            if keep {
                                // v's stake transfers into the slot.
                                product = __torajs_arr_push_any(
                                    product as *mut c_void,
                                    __torajs_anyv_unbox_tag(v) as u64,
                                    __torajs_anyv_unbox_value(v) as u64,
                                );
                            } else {
                                __torajs_value_drop_heap(v as *mut c_void);
                            }
                        }
                        // forEach — side effects only.
                        _ => {
                            __torajs_value_drop_heap(r as *mut c_void);
                            __torajs_value_drop_heap(v as *mut c_void);
                        }
                    }
                    k += 1;
                }
                match mid {
                    mm if mm == ANY_METHOD_EVERY => VALUE_TRUE,
                    mm if mm == ANY_METHOD_SOME => VALUE_FALSE,
                    mm if mm == ANY_METHOD_MAP
                        || mm == ANY_METHOD_FILTER
                        || mm == ANY_METHOD_FLAT_MAP =>
                    {
                        __torajs_anyv_box_pointer(product as *mut c_void)
                    }
                    _ => VALUE_UNDEFINED,
                }
            }
        }
    }
}
