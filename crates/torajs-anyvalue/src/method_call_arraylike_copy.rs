//! Change-by-copy tail of the generic array-like family — the
//! per-[[Get]] materialize kernels shared by slice / join /
//! toReversed / toSorted, plus the toSpliced (§23.1.3.35) and with
//! (§23.1.3.39) arms whose read sets are exact: the spliced-out
//! range and the substituted index must NOT fire their getters,
//! and with's RangeError precedes any element Get. Split out of
//! [`crate::method_call_arraylike`] when the toSpliced/with arms
//! would have pushed it over the 500-line limit.
//!
//! Argument ledger mirrors the parent: argv slots are BORROWED
//! (items take a fresh stake before entering a product); per-index
//! Gets are OWNED and transfer into the product; returns follow the
//! owned boxed-value convention.

use core::ffi::c_void;

use crate::method_call::to_index;
use crate::method_call_arraylike::{arraylike_get, arraylike_has};
use crate::nanbox::{AnyValue, VALUE_UNDEFINED};
use crate::nanbox_encode::{
    __torajs_anyv_box_pointer, __torajs_anyv_unbox_tag, __torajs_anyv_unbox_value,
};

unsafe extern "C" {
    /// torajs-arr — fresh Array<Any> (the copy products).
    fn __torajs_arr_alloc_any(cap: u64) -> *mut u8;
    /// torajs-arr — append one (tag, value) pair; ANY_HEAP transfers
    /// ownership. Caller must capture the (possibly moved) return.
    fn __torajs_arr_push_any(arr: *mut c_void, tag: u64, value: u64) -> *mut u8;
    /// torajs-throw — pending-throw flag (1 = a throw is recorded).
    fn __torajs_throw_check() -> i64;
    /// torajs-throw — record a RangeError (with's OOB index).
    fn __torajs_throw_range_error(msg: *const core::ffi::c_char);
    /// torajs-throw — record a TypeError (toSpliced's length cap).
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    /// Cross-tier — universal NaN-box-safe heap-value release.
    fn __torajs_value_drop_heap(p: *mut c_void);
    /// torajs-arr — §23.1.3.13 flat with ToIntegerOrInfinity depth
    /// (fresh +1 rc out; depth ≤ 0 answers a plain copy).
    fn __torajs_arr_any_flat_depth(arr: *const u8, depth: i64) -> *mut u8;
}

/// `flat` over the generic receiver (§23.1.3.13) — the depthNum
/// decode (ToIntegerOrInfinity; its ToNumber throws on a Symbol) is
/// ordered after the caller's length read, before any element Get.
/// Has-gated collect (absent keys skip entirely — no undefined
/// filler), then the shared flat-depth kernel spreads array
/// elements; inner spreads read fresh Arr cells, no observable
/// getters fire past the top-level walk.
pub(crate) unsafe fn arraylike_flat(
    obj: *mut c_void,
    len: i64,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    unsafe {
        let depth_av = if argc > 0 { *argv } else { VALUE_UNDEFINED };
        let depth = to_index(depth_av, 1);
        if __torajs_throw_check() != 0 {
            return VALUE_UNDEFINED;
        }
        let mut tmp: *mut u8 = __torajs_arr_alloc_any(0);
        let mut k: i64 = 0;
        while k < len {
            if arraylike_has(obj, k) {
                tmp = append_gets(tmp, obj, k, k + 1);
                if __torajs_throw_check() != 0 {
                    __torajs_value_drop_heap(tmp as *mut c_void);
                    return VALUE_UNDEFINED;
                }
            }
            k += 1;
        }
        let p = __torajs_arr_any_flat_depth(tmp as *const u8, depth);
        __torajs_value_drop_heap(tmp as *mut c_void);
        __torajs_anyv_box_pointer(p as *mut c_void)
    }
}

/// Append `Get(O, lo) … Get(O, hi-1)` onto `dst` (each Get's stake
/// transfers into its slot), answering the possibly-moved dst. Stops
/// on an abrupt getter completion — Get propagates (§7.3.2), so
/// later indexes must not fire; the caller sees the pending throw
/// and owns the partial product.
pub(crate) unsafe fn append_gets(mut dst: *mut u8, obj: *mut c_void, lo: i64, hi: i64) -> *mut u8 {
    unsafe {
        let mut k = lo;
        while k < hi {
            let v = arraylike_get(obj, k);
            if __torajs_throw_check() != 0 {
                __torajs_value_drop_heap(v as *mut c_void);
                return dst;
            }
            dst = __torajs_arr_push_any(
                dst as *mut c_void,
                __torajs_anyv_unbox_tag(v) as u64,
                __torajs_anyv_unbox_value(v) as u64,
            );
            k += 1;
        }
        dst
    }
}

/// `[Get(O, lo) … Get(O, hi-1)]` as a fresh owned Array<Any> —
/// slice's product and join's temp.
pub(crate) unsafe fn materialize_range(obj: *mut c_void, lo: i64, hi: i64) -> *mut u8 {
    unsafe {
        // Cap hint only (push grows on demand) — a ToLength-sized
        // range must not size the malloc.
        let dst = __torajs_arr_alloc_any((hi - lo).clamp(0, 4096) as u64);
        append_gets(dst, obj, lo, hi)
    }
}

/// `[Get(O, len-1) … Get(O, 0)]` as a fresh owned Array<Any> —
/// toReversed's product ([`materialize_range`]'s descending twin;
/// the §23.1.3.33 read order is observable through getters).
pub(crate) unsafe fn materialize_range_rev(obj: *mut c_void, len: i64) -> *mut u8 {
    unsafe {
        let mut dst = __torajs_arr_alloc_any(len.clamp(0, 4096) as u64);
        let mut k = 0;
        while k < len {
            let v = arraylike_get(obj, len - k - 1);
            dst = __torajs_arr_push_any(
                dst as *mut c_void,
                __torajs_anyv_unbox_tag(v) as u64,
                __torajs_anyv_unbox_value(v) as u64,
            );
            k += 1;
        }
        dst
    }
}

/// §23.1.3.35 toSpliced over a generic host — both
/// ToIntegerOrInfinity coercions run (and may throw) before any
/// element Get; the product reads exactly `[0, actualStart)` and
/// `[actualStart+skip, len)` (the removed range's getters never
/// fire). `obj` may be NULL only when `len == 0` (the empty-receiver
/// arm — no Get ever dereferences it).
pub(crate) unsafe fn arraylike_to_spliced(
    obj: *mut c_void,
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
        let rel = to_index(arg_at(0), 0);
        if __torajs_throw_check() != 0 {
            return VALUE_UNDEFINED;
        }
        let actual_start = if rel < 0 {
            (rel + len).max(0)
        } else {
            rel.min(len)
        };
        // §23.1.3.35 steps 6-8 — argc-dependent skip normalization
        // (absent start → 0, absent skipCount → to end).
        let actual_skip = if argc == 0 {
            0
        } else if argc == 1 {
            len - actual_start
        } else {
            let sc = to_index(arg_at(1), 0);
            if __torajs_throw_check() != 0 {
                return VALUE_UNDEFINED;
            }
            sc.clamp(0, len - actual_start)
        };
        let item_count = (argc - 2).max(0);
        // Step 10 — the 2^53-1 product-length cap throws BEFORE any
        // element read.
        let new_len = len + item_count - actual_skip;
        if new_len > 9007199254740991 {
            __torajs_throw_type_error(c"Invalid array length".as_ptr());
            return VALUE_UNDEFINED;
        }
        // Step 11 — ArrayCreate(newLen) then caps at 2^32-1
        // (RangeError; ordered after the step-10 TypeError, before
        // any element read).
        if new_len > 4294967295 {
            __torajs_throw_range_error(
                c"Array length must be a positive integer of safe magnitude.".as_ptr(),
            );
            return VALUE_UNDEFINED;
        }
        let mut dst = __torajs_arr_alloc_any(new_len.clamp(0, 4096) as u64);
        dst = append_gets(dst, obj, 0, actual_start);
        if __torajs_throw_check() != 0 {
            __torajs_value_drop_heap(dst as *mut c_void);
            return VALUE_UNDEFINED;
        }
        let mut i = 0;
        while i < item_count {
            // Borrowed argv slot → fresh stake into the product.
            let it = *argv.add((2 + i) as usize);
            crate::nanbox_ffi::__torajs_anyv_rc_inc(it);
            dst = __torajs_arr_push_any(
                dst as *mut c_void,
                __torajs_anyv_unbox_tag(it) as u64,
                __torajs_anyv_unbox_value(it) as u64,
            );
            i += 1;
        }
        dst = append_gets(dst, obj, actual_start + actual_skip, len);
        if __torajs_throw_check() != 0 {
            __torajs_value_drop_heap(dst as *mut c_void);
            return VALUE_UNDEFINED;
        }
        __torajs_anyv_box_pointer(dst as *mut c_void)
    }
}

/// §23.1.3.39 with over a generic host — ToIntegerOrInfinity then
/// the RangeError gate, both BEFORE any element Get; the substituted
/// index's getter never fires. `obj` may be NULL only when
/// `len == 0` (the RangeError always fires first there).
pub(crate) unsafe fn arraylike_with(
    obj: *mut c_void,
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
        let rel = to_index(arg_at(0), 0);
        if __torajs_throw_check() != 0 {
            return VALUE_UNDEFINED;
        }
        let actual = if rel < 0 { rel + len } else { rel };
        if actual < 0 || actual >= len {
            __torajs_throw_range_error(c"Invalid index".as_ptr());
            return VALUE_UNDEFINED;
        }
        // §23.1.3.39 step 7 — ArrayCreate(len) throws RangeError
        // above 2^32-1 (ordered after the step-5 index check, before
        // any element read).
        if len > 4294967295 {
            __torajs_throw_range_error(
                c"Array length must be a positive integer of safe magnitude.".as_ptr(),
            );
            return VALUE_UNDEFINED;
        }
        let mut dst = __torajs_arr_alloc_any(len.clamp(0, 4096) as u64);
        dst = append_gets(dst, obj, 0, actual);
        if __torajs_throw_check() != 0 {
            __torajs_value_drop_heap(dst as *mut c_void);
            return VALUE_UNDEFINED;
        }
        // Borrowed value arg → fresh stake into the product (never
        // coerced per spec).
        let v = arg_at(1);
        crate::nanbox_ffi::__torajs_anyv_rc_inc(v);
        dst = __torajs_arr_push_any(
            dst as *mut c_void,
            __torajs_anyv_unbox_tag(v) as u64,
            __torajs_anyv_unbox_value(v) as u64,
        );
        dst = append_gets(dst, obj, actual + 1, len);
        if __torajs_throw_check() != 0 {
            __torajs_value_drop_heap(dst as *mut c_void);
            return VALUE_UNDEFINED;
        }
        __torajs_anyv_box_pointer(dst as *mut c_void)
    }
}
