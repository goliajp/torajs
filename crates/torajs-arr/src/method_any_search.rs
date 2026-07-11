//! `any`-receiver Array search / join glue (Any-method-call RFC
//! 20260704 C2) — `a.indexOf(x)` / `a.includes(x)` / `a.join(sep)`
//! where `a` crossed into the `any` world.
//!
//! Read-only siblings of [`crate::method_any`]'s mutators. The
//! search pair runs the S3-get kernel loop
//! ([`crate::index_any::__torajs_arr_index_get`], kind-aware boxed
//! read) with per-slot `===` via the cross-tier
//! `__torajs_anyv_strict_eq`; join dispatches on the receiver's
//! element kind onto the existing typed join kernels — a HEAP-kind
//! slot's raw pointer bits ARE its NaN-box cell encoding, so the
//! HEAP tier shares [`crate::join::__torajs_arr_join_any`]'s
//! ToString walk (which is deque-head-aware) with Arr&lt;Any&gt;.
//!
//! Argument ledger: `needle` / `sep` are BORROWED from the
//! dispatcher (torajs-anyvalue incs/drops around the call); returns
//! follow the boxed-value convention.

use core::ffi::c_void;

use torajs_rc::{
    ARR_KIND_BOOL, ARR_KIND_F64, ARR_KIND_HEAP, ARR_KIND_I64, ARR_KIND_UNSET, FLAG_ARR_ANY,
    HeapHeader,
};

use crate::layout::ARR_LEN_OFF;

unsafe extern "C" {
    /// Cross-tier — torajs-anyvalue strict `===` (ES §7.2.13,
    /// including Str/Substr/ShortStr content equality).
    fn __torajs_anyv_strict_eq(l: u64, r: u64) -> bool;
    fn __torajs_anyv_unbox_tag(v: u64) -> i64;
    fn __torajs_anyv_unbox_value(v: u64) -> i64;
    /// Cross-tier — universal NaN-box-safe heap dropper (releases
    /// the +1 each `arr_index_get` read takes for cells).
    fn __torajs_value_drop_heap(p: *mut c_void);
}

/// `from`-argument normalization per ES §23.1.3.17 step 4-6:
/// negative wraps from the end, clamped to 0.
#[inline]
fn norm_from(from: i64, len: i64) -> i64 {
    if from < 0 { (len + from).max(0) } else { from }
}

/// NaN check on a boxed value (`includes`' SameValueZero vs
/// `indexOf`'s strict-eq is exactly the NaN row).
#[inline]
unsafe fn is_nan_boxed(v: u64) -> bool {
    unsafe {
        __torajs_anyv_unbox_tag(v) == 3
            && f64::from_bits(__torajs_anyv_unbox_value(v) as u64).is_nan()
    }
}

/// Shared search loop — answers the first index `>= from` whose
/// slot matches, or -1. `same_value_zero` adds the NaN row.
unsafe fn search(arr: *const c_void, needle: u64, from: i64, same_value_zero: bool) -> i64 {
    unsafe {
        let len = *((arr as *const u8).add(ARR_LEN_OFF) as *const u64) as i64;
        let needle_nan = same_value_zero && is_nan_boxed(needle);
        let mut i = norm_from(from, len);
        while i < len {
            let v = crate::index_any::__torajs_arr_index_get(arr, i);
            let hit = __torajs_anyv_strict_eq(v, needle) || (needle_nan && is_nan_boxed(v));
            __torajs_value_drop_heap(v as *mut c_void);
            if hit {
                return i;
            }
            i += 1;
        }
        -1
    }
}

/// `a.indexOf(x, from)` per ES §23.1.3.17 — strict-eq scan, found
/// index or -1 (never finds NaN).
///
/// # Safety
/// `arr` is a valid `Tag::Arr` heap pointer; `needle` is a live
/// borrowed AnyValue.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_any_index_of(
    arr: *const c_void,
    needle: u64,
    from: i64,
) -> i64 {
    unsafe { search(arr, needle, from, false) }
}

/// `a.lastIndexOf(x, from)` per ES §23.1.3.20 — strict-eq scan
/// BACKWARDS from `from` (§ step 4-6: `n >= len` clamps to the last
/// slot, negative wraps from the end, still-negative answers -1
/// without scanning), found index or -1 (never finds NaN — no
/// SameValueZero row on this method).
///
/// # Safety
/// See [`__torajs_arr_any_index_of`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_any_last_index_of(
    arr: *const c_void,
    needle: u64,
    from: i64,
) -> i64 {
    unsafe {
        let len = *((arr as *const u8).add(ARR_LEN_OFF) as *const u64) as i64;
        let mut i = if from >= len {
            len - 1
        } else if from < 0 {
            len + from
        } else {
            from
        };
        while i >= 0 {
            let v = crate::index_any::__torajs_arr_index_get(arr, i);
            let hit = __torajs_anyv_strict_eq(v, needle);
            __torajs_value_drop_heap(v as *mut c_void);
            if hit {
                return i;
            }
            i -= 1;
        }
        -1
    }
}

/// `a.includes(x, from)` per ES §23.1.3.16 — SameValueZero scan
/// (finds NaN), 1/0.
///
/// # Safety
/// See [`__torajs_arr_any_index_of`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_any_includes(
    arr: *const c_void,
    needle: u64,
    from: i64,
) -> i64 {
    unsafe {
        if search(arr, needle, from, true) >= 0 {
            1
        } else {
            0
        }
    }
}

/// `a.join(sep)` per ES §23.1.3.18 — element-kind dispatch onto the
/// typed join kernels. Returns a fresh rc=1 Str as raw AnyValue
/// bits.
///
/// # Safety
/// `arr` is a valid `Tag::Arr` heap pointer; `sep` a valid heap Str.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_any_join(arr: *const u8, sep: *const u8) -> u64 {
    unsafe {
        let header = &*(arr as *const HeapHeader);
        let out = if header.flags & FLAG_ARR_ANY != 0 {
            crate::join::__torajs_arr_join_any(arr, sep)
        } else {
            match header.arr_elem_kind() {
                ARR_KIND_I64 => crate::join::__torajs_arr_join_i64(arr, sep),
                ARR_KIND_F64 => crate::join::__torajs_arr_join_f64(arr, sep),
                ARR_KIND_BOOL => crate::join::__torajs_arr_join_bool(arr, sep),
                // Raw heap-pointer slots are valid NaN-box cell
                // encodings — the Any walk ToStrings each one
                // (Str / Substr / nested composites alike).
                ARR_KIND_HEAP => crate::join::__torajs_arr_join_any(arr, sep),
                kind => {
                    debug_assert!(
                        kind == ARR_KIND_UNSET,
                        "arr_any_join: invalid elem kind {kind}"
                    );
                    debug_assert!(
                        false,
                        "arr_any_join: UNSET elem kind — a typed-arr→Any \
                         boxing site missed __torajs_arr_mark_kind"
                    );
                    crate::str_bridge::str_alloc_pooled(0)
                }
            }
        };
        out as u64
    }
}
