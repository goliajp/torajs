//! Generic array-like receiver arm (RFC 20260712-array-generic-
//! receiver chunks 2+3a) — ES §23.1.3 "intentionally generic"
//! Array.prototype methods over a plain-object (`Tag::DynObj`)
//! receiver, reached two ways:
//!
//! - `Array.prototype.every.call(obj, …)` — the reified-cell
//!   `call` / `apply` short-circuit re-dispatches with NULL name
//!   bytes; the dynobj arm routes an array-family mid here BEFORE
//!   its own-property probe (the mid is authoritative, not a name).
//! - `obj.pop = Array.prototype.pop; obj.pop()` — the own-property
//!   probe resolves the interned reified cell; its carried mid
//!   re-enters here with the receiver instead of invoking the
//!   bare-entry TypeError.
//!
//! Semantics per spec: `len = ToLength(Get(O, "length"))` (an
//! accessor length runs its getter — observable, and ordered BEFORE
//! the callback-callability check), per-index `Get(O, ToString(k))`
//! through the kind-aware [`crate::index_any`] entry (accessor
//! getters run), `HasProperty` gating where the spec skips absent
//! keys (indexOf / lastIndexOf and the HOF family — the sibling).
//! ToNumber on an object length answers NaN → 0 (the crate's
//! recorded no-valueOf-dispatch boundary).
//!
//! Scope (3a): read family only — mutators (push / pop / splice /
//! sort …) are chunk 3b. `Tag::Obj` struct receivers route in via
//! the same call/apply re-dispatch gate (RFC 20260721 刀 8 G2a);
//! their interior writes go through the member-set dispatcher,
//! where growth rejects loud (the G10 posture).
//!
//! Argument ledger: argv slots are BORROWED; per-index Gets are
//! OWNED (dropped here or transferred into products); returns
//! follow the owned boxed-value convention.

use core::ffi::c_void;

use torajs_rc::{
    ANY_METHOD_AT, ANY_METHOD_EVERY, ANY_METHOD_FILTER, ANY_METHOD_FIND, ANY_METHOD_FIND_INDEX,
    ANY_METHOD_FIND_LAST, ANY_METHOD_FIND_LAST_INDEX, ANY_METHOD_FLAT, ANY_METHOD_FLAT_MAP,
    ANY_METHOD_FOR_EACH, ANY_METHOD_INCLUDES, ANY_METHOD_INDEX_OF, ANY_METHOD_JOIN,
    ANY_METHOD_LAST_INDEX_OF, ANY_METHOD_MAP, ANY_METHOD_REDUCE, ANY_METHOD_REDUCE_RIGHT,
    ANY_METHOD_SLICE, ANY_METHOD_SOME, ANY_METHOD_TO_REVERSED, ANY_METHOD_TO_SORTED,
    ANY_METHOD_TO_SPLICED, ANY_METHOD_TO_STRING, ANY_METHOD_WITH,
};

use crate::method_call::to_index;
use crate::method_call_arraylike_copy::{materialize_range, materialize_range_rev};
use crate::nanbox::{AnyValue, VALUE_UNDEFINED, is_double, is_undefined};
use crate::nanbox_encode::{
    __torajs_anyv_box_from_pair, __torajs_anyv_box_i64, __torajs_anyv_box_pointer,
};
use crate::nanbox_ffi::{__torajs_anyv_strict_eq, __torajs_anyv_to_number};

unsafe extern "C" {
    /// torajs-str — fresh Str from raw bytes (probe keys / ",").
    fn __torajs_str_alloc(src: *const u8, len: i64) -> *mut u8;
    /// torajs-str — release a heap Str reference.
    fn __torajs_str_drop(s: *mut c_void);
    /// torajs-dynobj — own-key membership probe.
    fn __torajs_dynobj_has(obj: *const c_void, key: *const c_void) -> i32;
    /// torajs-throw — pending-throw flag (1 = a throw is recorded).
    fn __torajs_throw_check() -> i64;
    /// torajs-throw — slice / toReversed §10.4.2.2 ArrayCreate cap
    /// (records the pending throw and returns).
    fn __torajs_throw_range_error(msg: *const core::ffi::c_char);
    /// torajs-arr — fresh Array<Any> (the slice / map products).
    fn __torajs_arr_alloc_any(cap: u64) -> *mut u8;
    /// torajs-arr — element-kind-dispatched join (fresh Str).
    fn __torajs_arr_any_join(arr: *const u8, sep: *const u8) -> u64;
    /// torajs-arr — in-place stable merge sort (boxed comparator or
    /// the §23.1.3.30.2 ToString default). Answers the receiver.
    fn __torajs_arr_any_sort(
        arr: *mut u8,
        cb_env: *mut c_void,
        cb_entry: u64,
        has_cb: i64,
    ) -> *mut u8;
    /// Cross-tier — universal NaN-box-safe heap-value release.
    fn __torajs_value_drop_heap(p: *mut c_void);
}

pub(crate) use crate::method_call_arraylike_host::{arraylike_get, arraylike_has, arraylike_len};

/// The mids the generic arm implements (3a read family + the 3b-1
/// mutators) — the dynobj routing gates on this, so an
/// unimplemented array mid keeps today's TypeError instead of a
/// silent wrong answer.
pub(crate) fn arraylike_supported(mid: i64) -> bool {
    if crate::method_call_arraylike_mut::arraylike_mut_supported(mid) {
        return true;
    }
    matches!(
        mid,
        ANY_METHOD_INDEX_OF
            | ANY_METHOD_LAST_INDEX_OF
            | ANY_METHOD_INCLUDES
            | ANY_METHOD_AT
            | ANY_METHOD_JOIN
            | ANY_METHOD_TO_STRING
            | ANY_METHOD_SLICE
            | ANY_METHOD_TO_REVERSED
            | ANY_METHOD_TO_SORTED
            | ANY_METHOD_TO_SPLICED
            | ANY_METHOD_WITH
            | ANY_METHOD_EVERY
            | ANY_METHOD_SOME
            | ANY_METHOD_FOR_EACH
            | ANY_METHOD_MAP
            | ANY_METHOD_FILTER
            | ANY_METHOD_FIND
            | ANY_METHOD_FIND_INDEX
            | ANY_METHOD_FIND_LAST
            | ANY_METHOD_FIND_LAST_INDEX
            | ANY_METHOD_REDUCE
            | ANY_METHOD_REDUCE_RIGHT
            | ANY_METHOD_FLAT
            | ANY_METHOD_FLAT_MAP
    )
}

/// A primitive receiver's array-like face (RFC 20260721 G2b/G2d) —
/// §23.1.3 step 1 ToObject(prim) mints the wrapper and the scan
/// runs over IT: the wrapper's own-miss length/index reads walk to
/// its prototype singleton's expando face (`Boolean.prototype[1] =
/// x; Boolean.prototype.length = 2`) and the %Object.prototype%
/// root, and the callback's O argument is the wrapper (`obj
/// instanceof Boolean`), which the old proto-as-host shape got
/// wrong. No installed `length` anywhere on the chain reads
/// ToLength 0 — the empty-receiver semantics by construction.
/// Callers exclude the mutator family, so the null recv_slot is
/// never dereferenced.
pub(crate) unsafe fn arraylike_on_minted_wrapper(
    recv: AnyValue,
    mid: i64,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    unsafe {
        let w = crate::to_object::__torajs_any_to_object(recv);
        let out = arraylike_method(
            crate::nanbox::as_void_ptr(w),
            mid,
            core::ptr::null_mut(),
            argv,
            argc,
        );
        __torajs_value_drop_heap(crate::nanbox::as_void_ptr(w));
        out
    }
}

/// Generic arm entry — dispatches the scan family here, the
/// callback family in the hof sibling, the mutators in the mut
/// sibling (which threads `recv_slot` for the dynobj relocation
/// writeback). `obj` is a live DynObj cell.
pub(crate) unsafe fn arraylike_method(
    obj: *mut c_void,
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
        // §23.1.3.34 step 1 — toSorted admits its comparator BEFORE
        // even the length Get (unlike the HOF family, whose
        // IsCallable check is ordered after len): a non-callable
        // comparefn must throw before the length getter's side
        // effects fire.
        let sorted_cb = if mid == ANY_METHOD_TO_SORTED {
            if is_undefined(arg_at(0)) {
                Some((core::ptr::null_mut(), 0u64, 0i64))
            } else {
                match crate::method_call::closure_boxed_entry(arg_at(0)) {
                    Some((env, entry)) => Some((env, entry, 1i64)),
                    None => return crate::method_call::not_callable(),
                }
            }
        } else {
            None
        };
        // §23.1.3 step 1-2 of every generic method: length first —
        // its getter side effects are observable even when a later
        // step throws.
        let Some(len) = arraylike_len(obj) else {
            return VALUE_UNDEFINED;
        };
        if crate::method_call_arraylike_mut::arraylike_mut_supported(mid) {
            return crate::method_call_arraylike_mut::arraylike_mut(
                obj, mid, len, recv_slot, argv, argc,
            );
        }
        match mid {
            m if m == ANY_METHOD_INDEX_OF || m == ANY_METHOD_INCLUDES => {
                let needle = arg_at(0);
                let from = to_index(arg_at(1), 0);
                // ToIntegerOrInfinity(fromIndex) valueOf may throw —
                // abort before any per-index Get side effects.
                if __torajs_throw_check() != 0 {
                    return VALUE_UNDEFINED;
                }
                let mut k = if from < 0 { (from + len).max(0) } else { from };
                let has_gated = m == ANY_METHOD_INDEX_OF;
                while k < len {
                    if !has_gated || arraylike_has(obj, k) {
                        let v = arraylike_get(obj, k);
                        if __torajs_throw_check() != 0 {
                            __torajs_value_drop_heap(v as *mut c_void);
                            return VALUE_UNDEFINED;
                        }
                        let hit = if has_gated {
                            __torajs_anyv_strict_eq(needle, v)
                        } else {
                            same_value_zero(needle, v)
                        };
                        __torajs_value_drop_heap(v as *mut c_void);
                        if hit {
                            return if has_gated {
                                __torajs_anyv_box_i64(k)
                            } else {
                                __torajs_anyv_box_from_pair(1, 1)
                            };
                        }
                    }
                    k += 1;
                }
                if has_gated {
                    __torajs_anyv_box_i64(-1)
                } else {
                    __torajs_anyv_box_from_pair(1, 0)
                }
            }
            m if m == ANY_METHOD_LAST_INDEX_OF => arraylike_last_index_of(obj, len, argv, argc),
            m if m == ANY_METHOD_AT => {
                let rel = to_index(arg_at(0), 0);
                let adj = if rel < 0 { rel + len } else { rel };
                if adj < 0 || adj >= len {
                    return VALUE_UNDEFINED;
                }
                arraylike_get(obj, adj)
            }
            m if m == ANY_METHOD_JOIN || m == ANY_METHOD_TO_STRING => {
                // Materialize the Gets into a fresh Array<Any> and
                // reuse the kind-dispatched join kernel.
                let tmp = materialize_range(obj, 0, len);
                let sep_av = arg_at(0);
                let sep = if m == ANY_METHOD_TO_STRING || is_undefined(sep_av) {
                    __torajs_str_alloc(c",".as_ptr() as *const u8, 1)
                } else {
                    crate::nanbox_ffi::__torajs_anyv_to_str(sep_av) as *mut u8
                };
                let out = __torajs_arr_any_join(tmp as *const u8, sep as *const u8);
                __torajs_str_drop(sep as *mut c_void);
                __torajs_value_drop_heap(tmp as *mut c_void);
                out
            }
            m if m == ANY_METHOD_SLICE => {
                let rel = to_index(arg_at(0), 0);
                let lo = if rel < 0 {
                    (rel + len).max(0)
                } else {
                    rel.min(len)
                };
                let rel_end = to_index(arg_at(1), i64::MAX);
                let hi = if rel_end < 0 {
                    (rel_end + len).max(0)
                } else {
                    rel_end.min(len)
                };
                // §23.1.3.28 step 8 — ArraySpeciesCreate(O, count)
                // throws RangeError above 2^32-1 before any element
                // read (O(1) early exit).
                if hi.max(lo) - lo > 4294967295 {
                    __torajs_throw_range_error(
                        c"Length exceeded the maximum array length".as_ptr(),
                    );
                    return VALUE_UNDEFINED;
                }
                __torajs_anyv_box_pointer(materialize_range(obj, lo, hi.max(lo)) as *mut c_void)
            }
            // §23.1.3.33 — Get(O, len-k-1) descending, product is a
            // fresh dense Array (holes read as undefined, no gaps).
            m if m == ANY_METHOD_TO_REVERSED => {
                // §23.1.3.33 step 3 — ArrayCreate(len) throws
                // RangeError above 2^32-1 before any element read.
                if len > 4294967295 {
                    __torajs_throw_range_error(
                        c"Array length must be a positive integer of safe magnitude.".as_ptr(),
                    );
                    return VALUE_UNDEFINED;
                }
                __torajs_anyv_box_pointer(materialize_range_rev(obj, len) as *mut c_void)
            }
            // §23.1.3.35 / §23.1.3.39 — the exact-read-set copy arms
            // (the sibling): the spliced-out range / substituted
            // index never fire their getters, and with's RangeError
            // precedes any element Get.
            m if m == ANY_METHOD_TO_SPLICED => {
                crate::method_call_arraylike_copy::arraylike_to_spliced(obj, len, argv, argc)
            }
            m if m == ANY_METHOD_WITH => {
                crate::method_call_arraylike_copy::arraylike_with(obj, len, argv, argc)
            }
            // §23.1.3.34 — ascending per-[[Get]] materialize (every
            // element read fires before the first comparefn call —
            // the order is observable), then the shared stable-sort
            // kernel over the fresh copy.
            m if m == ANY_METHOD_TO_SORTED => {
                let (cb_env, cb_entry, has_cb) = sorted_cb.unwrap();
                let tmp = materialize_range(obj, 0, len);
                if __torajs_throw_check() != 0 {
                    // An index getter threw mid-read — release the
                    // partial product's stake.
                    __torajs_value_drop_heap(tmp as *mut c_void);
                    return VALUE_UNDEFINED;
                }
                let sorted = __torajs_arr_any_sort(tmp, cb_env, cb_entry, has_cb);
                if __torajs_throw_check() != 0 {
                    // comparefn threw — the fresh copy's stake drops
                    // before the throw propagates.
                    __torajs_value_drop_heap(sorted as *mut c_void);
                    return VALUE_UNDEFINED;
                }
                __torajs_anyv_box_pointer(sorted as *mut c_void)
            }
            // §23.1.3.13 — has-gated collect + shared flat-depth
            // kernel (the copy sibling; depth decode ordered there).
            m if m == ANY_METHOD_FLAT => {
                crate::method_call_arraylike_copy::arraylike_flat(obj, len, argv, argc)
            }
            // Callback family — the hof sibling (callability check
            // ordered after the length read above, per spec).
            _ => crate::method_call_arraylike_hof::arraylike_hof(obj, mid, len, argv, argc),
        }
    }
}

/// §23.1.3.20 — descending has-gated strict-eq scan. Step 4: an
/// absent fromIndex starts at the last slot; a PRESENT undefined
/// is 0.
unsafe fn arraylike_last_index_of(
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
        let needle = arg_at(0);
        let from = if argc >= 2 {
            to_index(arg_at(1), 0)
        } else {
            len - 1
        };
        if __torajs_throw_check() != 0 {
            return VALUE_UNDEFINED;
        }
        let mut k = if from < 0 {
            from + len
        } else {
            from.min(len - 1)
        };
        while k >= 0 {
            if arraylike_has(obj, k) {
                let v = arraylike_get(obj, k);
                if __torajs_throw_check() != 0 {
                    __torajs_value_drop_heap(v as *mut c_void);
                    return VALUE_UNDEFINED;
                }
                let hit = __torajs_anyv_strict_eq(needle, v);
                __torajs_value_drop_heap(v as *mut c_void);
                if hit {
                    return __torajs_anyv_box_i64(k);
                }
            }
            k -= 1;
        }
        __torajs_anyv_box_i64(-1)
    }
}

/// SameValueZero (§7.2.9) — strict-eq plus NaN equals NaN.
unsafe fn same_value_zero(a: AnyValue, b: AnyValue) -> bool {
    unsafe {
        if __torajs_anyv_strict_eq(a, b) {
            return true;
        }
        let a_nan = is_double(a) && __torajs_anyv_to_number(a).is_nan();
        let b_nan = is_double(b) && __torajs_anyv_to_number(b).is_nan();
        a_nan && b_nan
    }
}
