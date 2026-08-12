//! §24.2.4 set-like walks — the seven ES2025 Set methods over a
//! [`crate::set_like::SetRecord`] argument. Each follows its spec
//! algorithm's side split: the receiver-side walk (index over the
//! live SetData, re-reading bounds every step so a mutating `has`
//! callback is observed, per the spec's re-read note) drives when
//! `thisSize ≤ record.size`, the keys()-iterator walk otherwise —
//! that asymmetry is itself observable (which of `has` / `keys`
//! gets called) and pinned by the test262 set-like-class-order
//! suites.
//!
//! Ownership ledger:
//! - `map_iter_next` hands BORROWED entry pairs — every walk bumps
//!   the element into an owned frame before calling out (`has` can
//!   delete the entry from under its own argv);
//! - `map_set` / `map_has` / `map_delete` CONSUME one key stake
//!   (collections pair ABI) — the walks hand a fresh
//!   `unbox_value_owned` bump and keep releasing their own;
//! - an early exit off the keys walk runs §7.4.9 IteratorClose
//!   (`return()` observable) before answering;
//! - a throw mid-walk drops the partial product and answers the
//!   kernel's sentinel (0 / NULL) with the throw pending.

use core::ffi::c_void;
use core::ptr::{null, null_mut};

use crate::iter_any_get_method::{generic_iter_close, generic_iter_step};
use crate::nanbox::{AnyValue, VALUE_UNDEFINED};
use crate::nanbox_encode::{
    __torajs_anyv_box_from_pair, __torajs_anyv_unbox_tag, __torajs_anyv_unbox_value_owned,
};
use crate::nanbox_ffi::__torajs_anyv_rc_dec;
use crate::set_like::{
    SetRecord, call_has, canonical, get_set_record, keys_iterator, release_record,
};

unsafe extern "C" {
    /// torajs-collections — the Set-storage kernels (pair ABI; heap
    /// keys are consumed, see module doc).
    fn __torajs_map_size(p: *const c_void) -> i64;
    fn __torajs_map_set(p: *mut c_void, kt: i64, kp: i64, vt: i64, vp: i64);
    fn __torajs_map_has(p: *const c_void, kt: i64, kp: i64) -> i64;
    fn __torajs_map_delete(p: *mut c_void, kt: i64, kp: i64) -> i64;
    /// Caller-managed-cursor live walk (`*cursor = -1` first call;
    /// out pairs are borrows; bounds re-read every step).
    fn __torajs_map_iter_next(
        p: *const c_void,
        cursor: *mut i64,
        out_k_tag: *mut i64,
        out_k_payload: *mut i64,
        out_v_tag: *mut i64,
        out_v_payload: *mut i64,
    ) -> i64;
    /// Fresh empty Set / the clone face (`union(this, NULL)` walks
    /// `this` into a fresh rc-1 Set — the `new Set(src)` shape).
    fn __torajs_set_create() -> *mut c_void;
    fn __torajs_set_union(this: *const c_void, other: *const c_void) -> *mut c_void;
    /// Cross-tier — universal NaN-box-safe heap dropper (partial
    /// products on the throw path).
    fn __torajs_value_drop_heap(p: *mut c_void);
    fn __torajs_throw_check() -> i64;
}

/// isSubsetOf / isSupersetOf / isDisjointFrom over a set-like
/// argument. `op`: 0 = isSubsetOf (§24.2.4.6), 1 = isSupersetOf
/// (§24.2.4.7), 2 = isDisjointFrom (§24.2.4.5). Answers 1/0; a
/// pending throw answers 0 with the throw recorded.
///
/// # Safety
/// `this` is a live Set cell; `other` is a live borrowed AnyValue.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_set_relation_setlike(
    this: *mut c_void,
    other: AnyValue,
    op: i64,
) -> i64 {
    unsafe {
        let Some(rec) = get_set_record(other) else {
            return 0;
        };
        let this_size = __torajs_map_size(this) as f64;
        let out = match op {
            0 => {
                if this_size > rec.size {
                    Some(false)
                } else {
                    this_walk_has(this, &rec, true)
                }
            }
            1 => {
                if this_size < rec.size {
                    Some(false)
                } else {
                    keys_walk_probe(this, &rec, true)
                }
            }
            _ => {
                if this_size <= rec.size {
                    this_walk_has(this, &rec, false)
                } else {
                    keys_walk_probe(this, &rec, false)
                }
            }
        };
        release_record(rec);
        match out {
            Some(true) => 1,
            _ => 0,
        }
    }
}

/// union / intersection / difference / symmetricDifference over a
/// set-like argument. `op`: 0 = union (§24.2.4.10), 1 = intersection
/// (§24.2.4.4), 2 = difference (§24.2.4.3), 3 = symmetricDifference
/// (§24.2.4.8). Answers a fresh rc-1 Set; NULL = a pending throw.
///
/// # Safety
/// `this` is a live Set cell; `other` is a live borrowed AnyValue.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_set_setop_setlike(
    this: *mut c_void,
    other: AnyValue,
    op: i64,
) -> *mut c_void {
    unsafe {
        let Some(rec) = get_set_record(other) else {
            return null_mut();
        };
        let this_size = __torajs_map_size(this) as f64;
        let result = match op {
            0 => op_union(this, &rec),
            1 => op_intersection(this, this_size, &rec),
            2 => op_difference(this, this_size, &rec),
            _ => op_symmetric_difference(this, &rec),
        };
        release_record(rec);
        result.unwrap_or(null_mut())
    }
}

/// Receiver-side walk: every live element through `Call(has, «e»)`,
/// refusing on the first answer ≠ `want` (want=true → isSubsetOf,
/// want=false → isDisjointFrom's small side). `None` = threw.
unsafe fn this_walk_has(this: *mut c_void, rec: &SetRecord, want: bool) -> Option<bool> {
    unsafe {
        let mut cursor = -1i64;
        let (mut kt, mut kv, mut vt, mut vv) = (0i64, 0i64, 0i64, 0i64);
        while __torajs_map_iter_next(this, &mut cursor, &mut kt, &mut kv, &mut vt, &mut vv) != 0 {
            // Owned frame over the borrowed pair — see module doc.
            crate::payload_rc_inc(kt, kv);
            let e = __torajs_anyv_box_from_pair(kt, kv);
            let verdict = call_has(rec, e);
            __torajs_anyv_rc_dec(e);
            match verdict {
                None => return None,
                Some(b) if b != want => return Some(false),
                _ => {}
            }
        }
        Some(true)
    }
}

/// keys()-iterator walk probing the receiver: refuses on the first
/// membership verdict ≠ `want` (want=true → isSupersetOf, want=false
/// → isDisjointFrom's large side), closing the iterator per §7.4.9.
unsafe fn keys_walk_probe(this: *mut c_void, rec: &SetRecord, want: bool) -> Option<bool> {
    unsafe {
        let it = keys_iterator(rec)?;
        loop {
            let mut out = VALUE_UNDEFINED;
            if generic_iter_step(it, &mut out, false) == 0 {
                __torajs_anyv_rc_dec(it);
                if __torajs_throw_check() != 0 {
                    return None;
                }
                return Some(true);
            }
            let v = canonical(out);
            let kt = __torajs_anyv_unbox_tag(v);
            let kp = __torajs_anyv_unbox_value_owned(v);
            let hit = __torajs_map_has(this, kt, kp) != 0;
            __torajs_anyv_rc_dec(v);
            if hit != want {
                generic_iter_close(it);
                __torajs_anyv_rc_dec(it);
                if __torajs_throw_check() != 0 {
                    return None;
                }
                return Some(false);
            }
        }
    }
}

/// §24.2.4.10 union — clone `this`, append every keys() element
/// (canonicalized; `map_set` overwrite keeps first-insert order).
unsafe fn op_union(this: *mut c_void, rec: &SetRecord) -> Option<*mut c_void> {
    unsafe {
        let result = __torajs_set_union(this, null());
        let Some(it) = keys_iterator(rec) else {
            __torajs_value_drop_heap(result);
            return None;
        };
        loop {
            let mut out = VALUE_UNDEFINED;
            if generic_iter_step(it, &mut out, false) == 0 {
                __torajs_anyv_rc_dec(it);
                if __torajs_throw_check() != 0 {
                    __torajs_value_drop_heap(result);
                    return None;
                }
                return Some(result);
            }
            let v = canonical(out);
            let kt = __torajs_anyv_unbox_tag(v);
            let kp = __torajs_anyv_unbox_value_owned(v);
            __torajs_map_set(result, kt, kp, 5, 0);
            __torajs_anyv_rc_dec(v);
        }
    }
}

/// §24.2.4.4 intersection — small side drives: the receiver walk
/// keeps `has`-approved elements, the keys walk keeps elements the
/// receiver holds. Fresh result either way.
unsafe fn op_intersection(
    this: *mut c_void,
    this_size: f64,
    rec: &SetRecord,
) -> Option<*mut c_void> {
    unsafe {
        let result = __torajs_set_create();
        if this_size <= rec.size {
            let mut cursor = -1i64;
            let (mut kt, mut kv, mut vt, mut vv) = (0i64, 0i64, 0i64, 0i64);
            while __torajs_map_iter_next(this, &mut cursor, &mut kt, &mut kv, &mut vt, &mut vv) != 0
            {
                crate::payload_rc_inc(kt, kv);
                let e = __torajs_anyv_box_from_pair(kt, kv);
                match call_has(rec, e) {
                    None => {
                        __torajs_anyv_rc_dec(e);
                        __torajs_value_drop_heap(result);
                        return None;
                    }
                    Some(true) => {
                        let ekt = __torajs_anyv_unbox_tag(e);
                        let ekp = __torajs_anyv_unbox_value_owned(e);
                        __torajs_map_set(result, ekt, ekp, 5, 0);
                    }
                    Some(false) => {}
                }
                __torajs_anyv_rc_dec(e);
            }
        } else {
            let Some(it) = keys_iterator(rec) else {
                __torajs_value_drop_heap(result);
                return None;
            };
            loop {
                let mut out = VALUE_UNDEFINED;
                if generic_iter_step(it, &mut out, false) == 0 {
                    __torajs_anyv_rc_dec(it);
                    if __torajs_throw_check() != 0 {
                        __torajs_value_drop_heap(result);
                        return None;
                    }
                    break;
                }
                let v = canonical(out);
                let kt = __torajs_anyv_unbox_tag(v);
                let kp = __torajs_anyv_unbox_value_owned(v);
                if __torajs_map_has(this, kt, kp) != 0 {
                    let kp2 = __torajs_anyv_unbox_value_owned(v);
                    __torajs_map_set(result, kt, kp2, 5, 0);
                }
                __torajs_anyv_rc_dec(v);
            }
        }
        Some(result)
    }
}

/// §24.2.4.3 difference — clone `this`, then remove: the receiver
/// walk deletes `has`-approved elements, the keys walk deletes what
/// the iterator yields.
unsafe fn op_difference(this: *mut c_void, this_size: f64, rec: &SetRecord) -> Option<*mut c_void> {
    unsafe {
        let result = __torajs_set_union(this, null());
        if this_size <= rec.size {
            let mut cursor = -1i64;
            let (mut kt, mut kv, mut vt, mut vv) = (0i64, 0i64, 0i64, 0i64);
            while __torajs_map_iter_next(result, &mut cursor, &mut kt, &mut kv, &mut vt, &mut vv)
                != 0
            {
                crate::payload_rc_inc(kt, kv);
                let e = __torajs_anyv_box_from_pair(kt, kv);
                match call_has(rec, e) {
                    None => {
                        __torajs_anyv_rc_dec(e);
                        __torajs_value_drop_heap(result);
                        return None;
                    }
                    Some(true) => {
                        let ekt = __torajs_anyv_unbox_tag(e);
                        let ekp = __torajs_anyv_unbox_value_owned(e);
                        let _ = __torajs_map_delete(result, ekt, ekp);
                    }
                    Some(false) => {}
                }
                __torajs_anyv_rc_dec(e);
            }
        } else {
            let Some(it) = keys_iterator(rec) else {
                __torajs_value_drop_heap(result);
                return None;
            };
            loop {
                let mut out = VALUE_UNDEFINED;
                if generic_iter_step(it, &mut out, false) == 0 {
                    __torajs_anyv_rc_dec(it);
                    if __torajs_throw_check() != 0 {
                        __torajs_value_drop_heap(result);
                        return None;
                    }
                    break;
                }
                let v = canonical(out);
                let kt = __torajs_anyv_unbox_tag(v);
                let kp = __torajs_anyv_unbox_value_owned(v);
                let _ = __torajs_map_delete(result, kt, kp);
                __torajs_anyv_rc_dec(v);
            }
        }
        Some(result)
    }
}

/// §24.2.4.8 symmetricDifference — clone `this`, then toggle per
/// keys() element: a member of the ORIGINAL receiver is removed
/// from the result, a non-member is appended.
unsafe fn op_symmetric_difference(this: *mut c_void, rec: &SetRecord) -> Option<*mut c_void> {
    unsafe {
        let result = __torajs_set_union(this, null());
        let Some(it) = keys_iterator(rec) else {
            __torajs_value_drop_heap(result);
            return None;
        };
        loop {
            let mut out = VALUE_UNDEFINED;
            if generic_iter_step(it, &mut out, false) == 0 {
                __torajs_anyv_rc_dec(it);
                if __torajs_throw_check() != 0 {
                    __torajs_value_drop_heap(result);
                    return None;
                }
                return Some(result);
            }
            let v = canonical(out);
            let kt = __torajs_anyv_unbox_tag(v);
            let kp = __torajs_anyv_unbox_value_owned(v);
            let in_this = __torajs_map_has(this, kt, kp) != 0;
            let kp2 = __torajs_anyv_unbox_value_owned(v);
            if in_this {
                let _ = __torajs_map_delete(result, kt, kp2);
            } else {
                __torajs_map_set(result, kt, kp2, 5, 0);
            }
            __torajs_anyv_rc_dec(v);
        }
    }
}

/// The any-tier dispatch face over the two kernels above — maps the
/// `ANY_METHOD_*` setops id onto its (kernel, op) pair and boxes the
/// answer per the dispatcher's conventions (a fresh Set cell rides
/// as its pointer bits; a predicate rides as a boxed bool). The
/// caller runs the throw check.
///
/// # Safety
/// `this` is a live Set cell; `other` is a live borrowed AnyValue.
pub(crate) unsafe fn setlike_method(this: *mut c_void, mid: i64, other: AnyValue) -> AnyValue {
    use torajs_rc::{
        ANY_METHOD_DIFFERENCE, ANY_METHOD_INTERSECTION, ANY_METHOD_IS_DISJOINT_FROM,
        ANY_METHOD_IS_SUBSET_OF, ANY_METHOD_IS_SUPERSET_OF, ANY_METHOD_SYMMETRIC_DIFFERENCE,
        ANY_METHOD_UNION,
    };
    unsafe {
        let setop = match mid {
            m if m == ANY_METHOD_UNION => Some(0),
            m if m == ANY_METHOD_INTERSECTION => Some(1),
            m if m == ANY_METHOD_DIFFERENCE => Some(2),
            m if m == ANY_METHOD_SYMMETRIC_DIFFERENCE => Some(3),
            _ => None,
        };
        if let Some(op) = setop {
            let s = __torajs_set_setop_setlike(this, other, op);
            if s.is_null() {
                return VALUE_UNDEFINED;
            }
            return s as u64;
        }
        let op = if mid == ANY_METHOD_IS_SUBSET_OF {
            0
        } else if mid == ANY_METHOD_IS_SUPERSET_OF {
            1
        } else {
            debug_assert!(mid == ANY_METHOD_IS_DISJOINT_FROM);
            2
        };
        __torajs_anyv_box_from_pair(1, __torajs_set_relation_setlike(this, other, op))
    }
}
