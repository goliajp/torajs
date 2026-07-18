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
//! sort …) are chunk 3b; `Tag::Obj` struct receivers stay on the
//! plain TypeError (RFC backlog).
//!
//! Argument ledger: argv slots are BORROWED; per-index Gets are
//! OWNED (dropped here or transferred into products); returns
//! follow the owned boxed-value convention.

use core::ffi::c_void;

use torajs_rc::{
    ANY_METHOD_AT, ANY_METHOD_EVERY, ANY_METHOD_FILTER, ANY_METHOD_FIND, ANY_METHOD_FIND_INDEX,
    ANY_METHOD_FIND_LAST, ANY_METHOD_FIND_LAST_INDEX, ANY_METHOD_FOR_EACH, ANY_METHOD_INCLUDES,
    ANY_METHOD_INDEX_OF, ANY_METHOD_JOIN, ANY_METHOD_LAST_INDEX_OF, ANY_METHOD_MAP,
    ANY_METHOD_REDUCE, ANY_METHOD_REDUCE_RIGHT, ANY_METHOD_SLICE, ANY_METHOD_SOME,
    ANY_METHOD_TO_STRING,
};

use crate::index_any::__torajs_any_index_get;
use crate::method_call::to_index;
use crate::nanbox::{AnyValue, VALUE_UNDEFINED, is_double, is_undefined};
use crate::nanbox_encode::{
    __torajs_anyv_box_from_pair, __torajs_anyv_box_i64, __torajs_anyv_box_pointer,
    __torajs_anyv_unbox_tag, __torajs_anyv_unbox_value,
};
use crate::nanbox_ffi::{__torajs_anyv_strict_eq, __torajs_anyv_to_number};

unsafe extern "C" {
    /// torajs-str — fresh Str from raw bytes (probe keys / ",").
    fn __torajs_str_alloc(src: *const u8, len: i64) -> *mut u8;
    /// torajs-str — release a heap Str reference.
    fn __torajs_str_drop(s: *mut c_void);
    /// torajs-dynobj — own-property probe pair ((5, 0) = absent).
    fn __torajs_dynobj_get_tag(obj: *const c_void, key: *const c_void) -> u64;
    fn __torajs_dynobj_get_value(obj: *const c_void, key: *const c_void) -> u64;
    /// torajs-dynobj — run an accessor entry's getter (owned answer).
    fn __torajs_accessor_invoke_getter(pair: *const c_void, recv_anyv: u64) -> u64;
    /// torajs-throw — pending-throw flag (1 = a throw is recorded).
    fn __torajs_throw_check() -> i64;
    /// torajs-arr — fresh Array<Any> (the slice / map products).
    fn __torajs_arr_alloc_any(cap: u64) -> *mut u8;
    /// torajs-arr — append one (tag, value) pair; ANY_HEAP transfers
    /// ownership. Caller must capture the (possibly moved) return.
    fn __torajs_arr_push_any(arr: *mut c_void, tag: u64, value: u64) -> *mut u8;
    /// torajs-arr — element-kind-dispatched join (fresh Str).
    fn __torajs_arr_any_join(arr: *const u8, sep: *const u8) -> u64;
    /// Cross-tier — universal NaN-box-safe heap-value release.
    fn __torajs_value_drop_heap(p: *mut c_void);
}

/// Accessor-entry sentinel in the dynobj probe's tag channel —
/// mirror of `method_call_dynobj.rs::ANY_ACCESSOR_TAG`.
const ANY_ACCESSOR_TAG: u64 = 6;

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
    )
}

/// `ToLength(Get(O, "length"))` — the accessor getter runs
/// (observable per §23.1.3 step 2 tests); `None` when the getter
/// left a pending throw. Object lengths answer NaN → 0 (recorded
/// no-valueOf boundary).
pub(crate) unsafe fn arraylike_len(obj: *mut c_void) -> Option<i64> {
    unsafe {
        let key = __torajs_str_alloc(c"length".as_ptr() as *const u8, 6);
        // 刀 2 (RFC 20260714-t262-top-clusters) — a `Tag::Obj` anon
        // struct receiver (`{0: v, length: 2}` lowers static) reads
        // its `length` field through the class-layouts probe; absent
        // field answers the undefined pair (→ ToLength 0).
        let obj_tag = (obj.cast::<u8>().add(4) as *const u16).read();
        let (dtag, dval) = if obj_tag == torajs_rc::Tag::Obj as u16 {
            crate::struct_probe::struct_field_pair(obj, key as *const c_void).unwrap_or((5, 0))
        } else {
            (
                __torajs_dynobj_get_tag(obj, key as *const c_void),
                __torajs_dynobj_get_value(obj, key as *const c_void),
            )
        };
        __torajs_str_drop(key as *mut c_void);
        let av = if dtag == ANY_ACCESSOR_TAG {
            let got = __torajs_accessor_invoke_getter(
                dval as *const c_void,
                crate::nanbox_encode::__torajs_anyv_box_from_pair(4, obj as i64),
            );
            if __torajs_throw_check() != 0 {
                __torajs_value_drop_heap(got as *mut c_void);
                return None;
            }
            got
        } else {
            // Borrow pair — ToNumber below only reads, no stake.
            __torajs_anyv_box_from_pair(dtag as i64, dval as i64)
        };
        let n = __torajs_anyv_to_number(av);
        if dtag == ANY_ACCESSOR_TAG {
            __torajs_value_drop_heap(av as *mut c_void);
        }
        // §7.1.20 ToLength runs ToNumber — an object length's valueOf
        // may throw (15.4.4.14-5-30: that abrupt completion must
        // precede the fromIndex ToInteger side effects).
        if __torajs_throw_check() != 0 {
            return None;
        }
        // §7.1.20 ToLength — NaN/negative clamp 0, cap 2^53-1.
        let len = if n.is_nan() || n <= 0.0 {
            0
        } else if n >= 9007199254740991.0 {
            9007199254740991
        } else {
            n as i64
        };
        Some(len)
    }
}

/// `Get(O, ToString(k))` — owned answer (accessor getters run).
pub(crate) unsafe fn arraylike_get(obj: *mut c_void, k: i64) -> AnyValue {
    unsafe { __torajs_any_index_get(__torajs_anyv_box_pointer(obj), k) }
}

/// `HasProperty(O, ToString(k))` — the hole gate for the has-gated
/// families (§23.1.3.17 step 9.a etc.).
pub(crate) unsafe fn arraylike_has(obj: *mut c_void, k: i64) -> bool {
    unsafe {
        let mut buf = [0u8; 20];
        let mut i = buf.len();
        let mut m = k.max(0) as u64;
        loop {
            i -= 1;
            buf[i] = b'0' + (m % 10) as u8;
            m /= 10;
            if m == 0 {
                break;
            }
        }
        let key = __torajs_str_alloc(buf[i..].as_ptr(), (buf.len() - i) as i64);
        let hit = crate::prop_has::__torajs_any_prop_has(
            __torajs_anyv_box_pointer(obj),
            key as *const c_void,
        );
        __torajs_str_drop(key as *mut c_void);
        hit != 0
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
            m if m == ANY_METHOD_LAST_INDEX_OF => {
                let needle = arg_at(0);
                // §23.1.3.20 step 4 — absent fromIndex starts at the
                // last slot; a PRESENT undefined is 0.
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
                __torajs_anyv_box_pointer(materialize_range(obj, lo, hi.max(lo)) as *mut c_void)
            }
            // Callback family — the hof sibling (callability check
            // ordered after the length read above, per spec).
            _ => crate::method_call_arraylike_hof::arraylike_hof(obj, mid, len, argv, argc),
        }
    }
}

/// `[Get(O, lo) … Get(O, hi-1)]` as a fresh owned Array<Any> —
/// slice's product and join's temp (each Get's stake transfers into
/// the slot).
unsafe fn materialize_range(obj: *mut c_void, lo: i64, hi: i64) -> *mut u8 {
    unsafe {
        // Cap hint only (push grows on demand) — a ToLength-sized
        // range must not size the malloc.
        let mut dst = __torajs_arr_alloc_any((hi - lo).clamp(0, 4096) as u64);
        let mut k = lo;
        while k < hi {
            let v = arraylike_get(obj, k);
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

/// 刀 2 (RFC 20260714-t262-top-clusters) — ES generic array-like
/// semantics over a receiver with NO index surface at all (bool
/// primitives: ToObject(v) carries no own `length`, so
/// `ToLength(undefined) = 0` and every loop is empty). The HOF
/// family still runs the §23.1.3 IsCallable(callbackfn) check
/// before the (empty) loop — the throw order is observable. Reduce
/// without an initial value throws per §23.1.3.24 step 5. Mutators
/// are excluded by the caller (a primitive receiver cannot grow).
pub(crate) unsafe fn arraylike_empty(mid: i64, argv: *const u64, argc: i64) -> AnyValue {
    use torajs_rc::{ANY_METHOD_FLAT, ANY_METHOD_FLAT_MAP};
    let arg0 = if argc > 0 {
        unsafe { *argv }
    } else {
        VALUE_UNDEFINED
    };
    unsafe {
        match mid {
            m if m == ANY_METHOD_INDEX_OF
                || m == ANY_METHOD_LAST_INDEX_OF
                || m == ANY_METHOD_FIND_INDEX
                || m == ANY_METHOD_FIND_LAST_INDEX =>
            {
                if m == ANY_METHOD_FIND_INDEX || m == ANY_METHOD_FIND_LAST_INDEX {
                    if crate::method_call::closure_boxed_entry(arg0).is_none() {
                        return crate::method_call::not_callable();
                    }
                }
                __torajs_anyv_box_i64(-1)
            }
            m if m == ANY_METHOD_INCLUDES => __torajs_anyv_box_from_pair(1, 0),
            m if m == ANY_METHOD_EVERY || m == ANY_METHOD_SOME => {
                if crate::method_call::closure_boxed_entry(arg0).is_none() {
                    return crate::method_call::not_callable();
                }
                __torajs_anyv_box_from_pair(1, i64::from(m == ANY_METHOD_EVERY))
            }
            m if m == ANY_METHOD_FIND || m == ANY_METHOD_FIND_LAST || m == ANY_METHOD_FOR_EACH => {
                if crate::method_call::closure_boxed_entry(arg0).is_none() {
                    return crate::method_call::not_callable();
                }
                VALUE_UNDEFINED
            }
            m if m == ANY_METHOD_AT => VALUE_UNDEFINED,
            m if m == ANY_METHOD_MAP || m == ANY_METHOD_FILTER || m == ANY_METHOD_FLAT_MAP => {
                if crate::method_call::closure_boxed_entry(arg0).is_none() {
                    return crate::method_call::not_callable();
                }
                __torajs_anyv_box_pointer(__torajs_arr_alloc_any(0) as *mut c_void)
            }
            m if m == ANY_METHOD_SLICE || m == ANY_METHOD_FLAT => {
                __torajs_anyv_box_pointer(__torajs_arr_alloc_any(0) as *mut c_void)
            }
            m if m == ANY_METHOD_JOIN || m == ANY_METHOD_TO_STRING => {
                __torajs_anyv_box_pointer(__torajs_str_alloc(b"".as_ptr(), 0) as *mut c_void)
            }
            m if m == ANY_METHOD_REDUCE || m == ANY_METHOD_REDUCE_RIGHT => {
                if crate::method_call::closure_boxed_entry(arg0).is_none() {
                    return crate::method_call::not_callable();
                }
                if argc < 2 {
                    // §23.1.3.24 step 5 — empty + no initial value.
                    __torajs_throw_type_error_msg();
                    return VALUE_UNDEFINED;
                }
                let init = *argv.add(1);
                crate::nanbox_ffi::__torajs_anyv_rc_inc(init);
                init
            }
            _ => crate::method_call::method_no_such(),
        }
    }
}

unsafe fn __torajs_throw_type_error_msg() {
    unsafe extern "C" {
        fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    }
    unsafe {
        __torajs_throw_type_error(c"Reduce of empty array with no initial value".as_ptr());
    }
}
