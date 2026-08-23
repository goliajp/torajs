//! §23.2.3.29 `sort` / §23.2.3.34 `toSorted` for typed arrays
//! (RFC 20260823-typedarray-substrate 刀 5).
//!
//! These live on the any-lane side rather than in `torajs-buffer`
//! because that is the shape of the spec step list, not a
//! convenience: §23.2.3.29 step 6 reads every element into a List,
//! orders the List, and only then writes it back. The ordering is a
//! walk over AnyValues calling a user comparator — the boxed dual
//! entry, the receiver-first channel, the pending-throw check — and
//! all of that machinery is here. Only the brand-and-extent question
//! comes from the buffer crate.
//!
//! Two things separate this from `Array.prototype.sort`.
//!
//! The default comparator is NUMERIC (§23.2.4.7
//! CompareTypedArrayElements), not the ToString order arrays use, so
//! `[10, 9]` sorts to `[9, 10]` here and to `[10, 9]` there. And it
//! is a total order over values that have none: -0 sorts before +0,
//! and NaN sorts after everything, with all NaNs equal to each
//! other.
//!
//! The merge is hand-rolled and stable. `slice::sort_by` would be
//! both, but it panics when the comparison is not a consistent total
//! order — and a user comparator is free to be arbitrary, which in
//! an AOT binary would be an abort rather than a catchable error.

use core::cmp::Ordering;
use core::ffi::c_void;

use crate::loose_eq::bigint_ffi::__torajs_bigint_cmp;
use crate::method_call::not_callable;
use crate::method_call_closure_dispatch::{closure_boxed_entry, invoke_boxed, recv_first_shift};
use crate::nanbox::{
    AnyValue, VALUE_UNDEFINED, as_double, as_int32, as_void_ptr, is_cell, is_double, is_int32,
    is_undefined,
};

unsafe extern "C" {
    fn __torajs_throw_check() -> i64;
    fn __torajs_anyv_to_number(v: AnyValue) -> f64;
    fn __torajs_anyv_rc_dec(v: AnyValue);
    /// §23.2.4.4 at the ABI — the length, or -1 with a pending throw.
    fn __torajs_typedarray_validate(av: AnyValue) -> i64;
    fn __torajs_typedarray_create_same_type(av: AnyValue, len: i64) -> AnyValue;
    fn __torajs_typedarray_index_get(av: AnyValue, index: f64) -> AnyValue;
    fn __torajs_typedarray_index_set(av: AnyValue, index: f64, value: AnyValue);
    fn __torajs_anyv_rc_inc(v: AnyValue);
}

/// The comparator, resolved once.
enum Cmp {
    /// §23.2.4.7 — numeric, total, NaN last, -0 before +0.
    Default,
    User {
        env: *mut c_void,
        entry: u64,
    },
}

/// §23.2.4.7 CompareTypedArrayElements. Both sides are the same
/// content type, because they came out of the same view.
unsafe fn default_compare(a: AnyValue, b: AnyValue) -> Ordering {
    if is_cell(a) && is_cell(b) {
        // The two BigInt element types; mathematical order.
        let c = unsafe { __torajs_bigint_cmp(as_void_ptr(a), as_void_ptr(b)) };
        return c.cmp(&0);
    }
    let x = number_of(a);
    let y = number_of(b);
    if x.is_nan() {
        return if y.is_nan() {
            Ordering::Equal
        } else {
            Ordering::Greater
        };
    }
    if y.is_nan() {
        return Ordering::Less;
    }
    if x < y {
        return Ordering::Less;
    }
    if x > y {
        return Ordering::Greater;
    }
    // Equal as numbers, and the only pair left that the spec still
    // orders: -0 comes before +0.
    let xn = x == 0.0 && x.is_sign_negative();
    let yn = y == 0.0 && y.is_sign_negative();
    match (xn, yn) {
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        _ => Ordering::Equal,
    }
}

fn number_of(v: AnyValue) -> f64 {
    if is_int32(v) {
        f64::from(as_int32(v))
    } else if is_double(v) {
        as_double(v)
    } else {
        f64::NAN
    }
}

/// One comparison. `None` = the comparator threw and the walk stops.
///
/// # Safety
/// `a` and `b` are live AnyValues the caller keeps alive.
unsafe fn compare(cmp: &Cmp, a: AnyValue, b: AnyValue) -> Option<Ordering> {
    match cmp {
        Cmp::Default => Some(unsafe { default_compare(a, b) }),
        Cmp::User { env, entry } => unsafe {
            let s = recv_first_shift(*env);
            let mut args = [VALUE_UNDEFINED; 8];
            args[s] = a;
            args[s + 1] = b;
            let r = invoke_boxed(*env, *entry, args.as_ptr(), (2 + s) as i64);
            if __torajs_throw_check() != 0 {
                __torajs_anyv_rc_dec(r);
                return None;
            }
            let n = __torajs_anyv_to_number(r);
            __torajs_anyv_rc_dec(r);
            if __torajs_throw_check() != 0 {
                return None;
            }
            // §23.2.3.29 step 6's SortCompare: a NaN result is +0,
            // and only the SIGN of the number is consulted.
            Some(if n < 0.0 {
                Ordering::Less
            } else if n > 0.0 {
                Ordering::Greater
            } else {
                Ordering::Equal
            })
        },
    }
}

/// Bottom-up stable merge. Returns false when the comparator threw,
/// leaving `items` in some permutation the caller only drops.
///
/// # Safety
/// `items` holds live AnyValues.
unsafe fn merge_sort(items: &mut Vec<AnyValue>, cmp: &Cmp) -> bool {
    let n = items.len();
    if n < 2 {
        return true;
    }
    let mut buf = vec![VALUE_UNDEFINED; n];
    let mut width = 1;
    while width < n {
        let mut lo = 0;
        while lo < n {
            let mid = (lo + width).min(n);
            let hi = (lo + 2 * width).min(n);
            let (mut i, mut j, mut k) = (lo, mid, lo);
            while i < mid && j < hi {
                // `Greater` and not `>=` is what keeps it stable:
                // equal elements take the left one first.
                let Some(o) = (unsafe { compare(cmp, items[i], items[j]) }) else {
                    return false;
                };
                if o == Ordering::Greater {
                    buf[k] = items[j];
                    j += 1;
                } else {
                    buf[k] = items[i];
                    i += 1;
                }
                k += 1;
            }
            while i < mid {
                buf[k] = items[i];
                i += 1;
                k += 1;
            }
            while j < hi {
                buf[k] = items[j];
                j += 1;
                k += 1;
            }
            lo = hi;
        }
        items[..n].copy_from_slice(&buf[..n]);
        width *= 2;
    }
    true
}

/// §23.2.3.29 (`in_place`) and §23.2.3.34 (`toSorted`).
///
/// # Safety
/// `recv` is a live TypedArray AnyValue; `argv` holds `argc` live
/// AnyValues.
pub(crate) unsafe fn typedarray_sort(
    recv: AnyValue,
    argv: *const u64,
    argc: i64,
    in_place: bool,
) -> AnyValue {
    unsafe {
        let cmp_arg = if argc > 0 { *argv } else { VALUE_UNDEFINED };
        // Step 1 runs BEFORE the receiver is validated, so a
        // non-callable comparator on a detached array reports the
        // comparator, not the buffer.
        let cmp = if is_undefined(cmp_arg) {
            Cmp::Default
        } else {
            match closure_boxed_entry(cmp_arg) {
                Some((env, entry)) => Cmp::User { env, entry },
                None => return not_callable(),
            }
        };
        let len = __torajs_typedarray_validate(recv);
        if len < 0 {
            return VALUE_UNDEFINED;
        }
        let mut items: Vec<AnyValue> = Vec::with_capacity(len as usize);
        for i in 0..len {
            items.push(__torajs_typedarray_index_get(recv, i as f64));
        }
        let ok = merge_sort(&mut items, &cmp);
        let out = if !ok {
            VALUE_UNDEFINED
        } else if in_place {
            for (j, v) in items.iter().enumerate() {
                __torajs_typedarray_index_set(recv, j as f64, *v);
            }
            __torajs_anyv_rc_inc(recv);
            recv
        } else {
            let fresh = __torajs_typedarray_create_same_type(recv, len);
            if __torajs_throw_check() == 0 {
                for (j, v) in items.iter().enumerate() {
                    __torajs_typedarray_index_set(fresh, j as f64, *v);
                }
            }
            fresh
        };
        // The reads were owned — a BigInt element is a fresh cell.
        for v in items {
            __torajs_anyv_rc_dec(v);
        }
        out
    }
}
