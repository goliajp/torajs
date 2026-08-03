//! `Tag::Arr` receiver arm extension — ES2023 change-array-by-copy
//! family + at / toString / flat / flatMap / findLast /
//! findLastIndex (RFC 20260712-array-generic-receiver chunk 1).
//! Sibling of `method_call_arr.rs` (the base id-switch delegates its
//! mid-miss here before floating no-such).
//!
//! Argument ledger: identical to the base arm — argv slots are
//! BORROWED; every returned array is fresh (+1 rc) per the owned
//! boxed-value convention.

use core::ffi::c_void;

use torajs_rc::{
    ANY_METHOD_AT, ANY_METHOD_FIND_LAST, ANY_METHOD_FIND_LAST_INDEX, ANY_METHOD_FLAT,
    ANY_METHOD_FLAT_MAP, ANY_METHOD_TO_REVERSED, ANY_METHOD_TO_SORTED, ANY_METHOD_TO_SPLICED,
    ANY_METHOD_TO_STRING, ANY_METHOD_WITH,
};

use crate::index_any::MIRROR_ARR_LEN_OFF;
use crate::method_call::{closure_boxed_entry, method_no_such, not_callable, to_index};
use crate::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr, is_cell, is_undefined};
use crate::nanbox_encode::__torajs_anyv_box_pointer;

unsafe extern "C" {
    /// torajs-arr — kind-aware full-range copy (fresh +1 rc).
    fn __torajs_arr_any_slice(arr: *const u8, start: i64, end: i64) -> *mut u8;
    /// torajs-arr — in-place 8-byte-slot swap (element-type-agnostic).
    fn __torajs_arr_reverse(arr: *mut u8) -> *mut u8;
    /// torajs-arr — §23.1.3.33 descending per-[[Get]] product (fresh
    /// +1 rc; exotic receivers observe the spec read order).
    fn __torajs_arr_any_to_reversed(arr: *const u8) -> *mut u8;
    /// torajs-arr — in-place stable merge sort (boxed comparator or
    /// the §23.1.3.30.2 ToString default). Answers the receiver.
    fn __torajs_arr_any_sort(
        arr: *mut u8,
        cb_env: *mut c_void,
        cb_entry: u64,
        has_cb: i64,
    ) -> *mut u8;
    /// torajs-arr — remove + variadic insert; answers the fresh
    /// (+1 rc) removed array (empty on the typed-admit TypeError).
    fn __torajs_arr_any_splice(
        arr: *mut u8,
        actual_start: i64,
        actual_delete: i64,
        items: *const u64,
        item_count: i64,
    ) -> *mut u8;
    /// torajs-arr — flat with ToIntegerOrInfinity depth (fresh +1 rc).
    fn __torajs_arr_any_flat_depth(arr: *const u8, depth: i64) -> *mut u8;
    /// torajs-arr — fresh owned `Array<Any>` copy of any receiver
    /// shape (the toSpliced seed — its product is a plain array).
    fn __torajs_arr_any_owned_copy(arr: *const u8) -> *mut u8;
    /// torajs-arr — change-by-copy with (NULL after the RangeError).
    fn __torajs_arr_any_with(arr: *const u8, i: i64, v: u64) -> *mut u8;
    /// torajs-arr — backwards predicate walks (find_loop modes 4/5).
    fn __torajs_arr_any_find_last(
        arr: *const c_void,
        cb_env: *mut c_void,
        cb_entry: u64,
        this_arg: u64,
    ) -> u64;
    fn __torajs_arr_any_find_last_index(
        arr: *const c_void,
        cb_env: *mut c_void,
        cb_entry: u64,
        this_arg: u64,
    ) -> u64;
    /// torajs-arr — HOF map (boxed fresh Arr<Any>, undefined on the
    /// callback throw).
    fn __torajs_arr_any_map(
        arr: *const c_void,
        cb_env: *mut c_void,
        cb_entry: u64,
        this_arg: u64,
    ) -> u64;
    /// torajs-arr — borrowed whole-box slot read (kind-aware).
    fn __torajs_arr_get_any_boxed(arr: *const c_void, i: u64) -> u64;
    /// torajs-arr — element-kind-dispatched join (fresh Str).
    fn __torajs_arr_any_join(arr: *const u8, sep: *const u8) -> u64;
    /// torajs-str — fresh Str from raw bytes (the "," separator).
    fn __torajs_str_alloc(src: *const u8, len: i64) -> *mut u8;
    /// torajs-str — release a heap Str reference.
    fn __torajs_str_drop(s: *mut c_void);
    /// Cross-tier — universal NaN-box-safe heap-value release (the
    /// flatMap / toSpliced intermediates).
    fn __torajs_value_drop_heap(p: *mut c_void);
    /// torajs-throw — pending-throw flag (1 = a throw is recorded).
    fn __torajs_throw_check() -> i64;
}

/// `xs.flat(depth)` with a RUNTIME depth operand (SSA lowering's
/// non-literal lane) — ToIntegerOrInfinity decode (§23.1.3.13 step
/// 2: undefined → 1, NaN → 0, Symbol/BigInt leave a pending
/// TypeError through ToNumber), then the shared flat-depth kernel.
/// Answers a fresh owned Arr pointer; NULL = pending throw (the
/// caller's throw check unwinds before the value is consumed).
///
/// # Safety
/// `arr` is a valid `Tag::Arr` heap pointer; `depth_av` is a
/// NaN-box AnyValue.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_flat_runtime_depth(arr: *const u8, depth_av: u64) -> *mut u8 {
    unsafe {
        let depth = to_index(depth_av, 1);
        if __torajs_throw_check() != 0 {
            return core::ptr::null_mut();
        }
        __torajs_arr_any_flat_depth(arr, depth)
    }
}

/// Extension id-switch — see module doc. `arr` / `argv` contracts
/// mirror [`crate::method_call_arr::arr_method`].
pub(crate) unsafe fn arr_method_ext(
    arr: *mut c_void,
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
    unsafe {
        match mid {
            m if m == ANY_METHOD_AT => {
                // §23.1.3.1 — relative index, OOB answers undefined.
                let len = *((arr as *const u8).add(MIRROR_ARR_LEN_OFF) as *const u64) as i64;
                let rel = to_index(arg_at(0), 0);
                let adj = if rel < 0 { rel + len } else { rel };
                if adj < 0 || adj >= len {
                    return VALUE_UNDEFINED;
                }
                // The slot read is borrowed — take the owned-return
                // stake (NaN-box-safe inc no-ops on immediates).
                let av = __torajs_arr_get_any_boxed(arr, adj as u64);
                torajs_rc::__torajs_rc_inc(av as *mut c_void);
                av
            }
            m if m == ANY_METHOD_TO_STRING => {
                // §23.1.3.36 — delegates to join with the default
                // "," separator (mirror of the dispatcher's
                // toLocaleString Arr routing).
                let sep = __torajs_str_alloc(c",".as_ptr() as *const u8, 1);
                let out = __torajs_arr_any_join(arr as *const u8, sep as *const u8);
                __torajs_str_drop(sep as *mut c_void);
                out
            }
            m if m == ANY_METHOD_FLAT => {
                // §23.1.3.13 — default depth 1; Infinity saturates
                // to i64::MAX inside to_index and terminates through
                // the kernel's nested scan.
                let depth = to_index(arg_at(0), 1);
                let p = __torajs_arr_any_flat_depth(arr as *const u8, depth);
                __torajs_anyv_box_pointer(p as *mut c_void)
            }
            m if m == ANY_METHOD_FLAT_MAP => {
                let Some((cb_env, cb_entry)) = closure_boxed_entry(arg_at(0)) else {
                    return not_callable();
                };
                let mapped = __torajs_arr_any_map(arr, cb_env, cb_entry, arg_at(1));
                if !is_cell(mapped) {
                    // Callback threw — the pending throw propagates.
                    return mapped;
                }
                let mp = as_void_ptr(mapped);
                let p = __torajs_arr_any_flat_depth(mp as *const u8, 1);
                __torajs_value_drop_heap(mp);
                __torajs_anyv_box_pointer(p as *mut c_void)
            }
            m if m == ANY_METHOD_FIND_LAST || m == ANY_METHOD_FIND_LAST_INDEX => {
                let Some((cb_env, cb_entry)) = closure_boxed_entry(arg_at(0)) else {
                    return not_callable();
                };
                if m == ANY_METHOD_FIND_LAST {
                    __torajs_arr_any_find_last(arr, cb_env, cb_entry, arg_at(1))
                } else {
                    __torajs_arr_any_find_last_index(arr, cb_env, cb_entry, arg_at(1))
                }
            }
            m if m == ANY_METHOD_TO_REVERSED => {
                // §23.1.3.33 step 5 reads the source high→low — the
                // order is observable through accessor indexes, so an
                // exotic receiver rides the descending per-[[Get]]
                // kernel (slice would gather ascending and reverse
                // after, firing getters in the wrong order). Plain
                // receivers keep the copy + in-place swap (no
                // observable reads).
                let flags = (*(arr as *const torajs_rc::HeapHeader)).flags;
                let p = if flags & torajs_rc::FLAG_ARR_EXOTIC_INDEX != 0 {
                    __torajs_arr_any_to_reversed(arr as *const u8)
                } else {
                    let p = __torajs_arr_any_slice(arr as *const u8, 0, i64::MAX);
                    __torajs_arr_reverse(p);
                    p
                };
                __torajs_anyv_box_pointer(p as *mut c_void)
            }
            m if m == ANY_METHOD_TO_SORTED => {
                // §23.1.3.34 — comparator admit mirrors the base
                // arm's sort (resolve BEFORE the copy so a
                // non-callable doesn't leak the fresh array).
                let (cb_env, cb_entry, has_cb) = if is_undefined(arg_at(0)) {
                    (core::ptr::null_mut(), 0u64, 0i64)
                } else {
                    let Some((e, en)) = closure_boxed_entry(arg_at(0)) else {
                        return not_callable();
                    };
                    (e, en, 1)
                };
                let p = __torajs_arr_any_slice(arr as *const u8, 0, i64::MAX);
                let q = __torajs_arr_any_sort(p, cb_env, cb_entry, has_cb);
                // q IS the fresh copy (sort answers its receiver) —
                // already owned, no extra stake.
                __torajs_anyv_box_pointer(q as *mut c_void)
            }
            m if m == ANY_METHOD_TO_SPLICED => {
                // §23.1.3.35 — same argc-dependent start/skip
                // normalization as the base arm's splice, applied to
                // a fresh copy; the removed array is discarded.
                let len = *((arr as *const u8).add(MIRROR_ARR_LEN_OFF) as *const u64) as i64;
                let rel = to_index(arg_at(0), 0);
                let actual_start = if rel < 0 {
                    (rel + len).max(0)
                } else {
                    rel.min(len)
                };
                let actual_skip = if argc == 0 {
                    0
                } else if argc == 1 {
                    len - actual_start
                } else {
                    to_index(arg_at(1), 0).clamp(0, len - actual_start)
                };
                let (items, item_count) = if argc > 2 {
                    (argv.add(2), argc - 2)
                } else {
                    (core::ptr::null(), 0)
                };
                let p = __torajs_arr_any_owned_copy(arr as *const u8);
                let removed =
                    __torajs_arr_any_splice(p, actual_start, actual_skip, items, item_count);
                __torajs_value_drop_heap(removed as *mut c_void);
                __torajs_anyv_box_pointer(p as *mut c_void)
            }
            m if m == ANY_METHOD_WITH => {
                let p = __torajs_arr_any_with(arr as *const u8, to_index(arg_at(0), 0), arg_at(1));
                if p.is_null() {
                    // RangeError recorded in the kernel.
                    return VALUE_UNDEFINED;
                }
                __torajs_anyv_box_pointer(p as *mut c_void)
            }
            _ => method_no_such(),
        }
    }
}
