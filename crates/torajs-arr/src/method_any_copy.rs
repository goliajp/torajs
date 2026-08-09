//! ES2023 change-array-by-copy / ES2019 flat-depth kernels for
//! `any` receivers (RFC 20260712-array-generic-receiver chunk 1).
//!
//! The dispatch arms compose `arr_any_slice` copies with the
//! existing in-place kernels (`arr_reverse` / `any_sort` /
//! `any_splice`); the two entry points here own the pieces that need
//! slot-layout access — flat's nested-array scan + with's slot
//! write. Both answer FRESH (+1 rc) arrays; `with` answers NULL
//! after recording a catchable RangeError.

use core::ffi::c_void;

use torajs_rc::{FLAG_ARR_ANY, HeapHeader};

use crate::any::{ANY_HEAP, slot_anyvalue_ptr};
use crate::layout::{ARR_LEN_OFF, TAG_ARR};

unsafe extern "C" {
    /// Cross-tier — torajs-rc. NaN-box-safe refcount bump.
    fn __torajs_rc_inc(p: *mut c_void);
    /// Cross-tier — universal NaN-box-safe heap-value release.
    fn __torajs_value_drop_heap(p: *mut c_void);
    /// Cross-tier — torajs-anyvalue NaN-box unpack.
    fn __torajs_anyv_unbox_tag(v: u64) -> i64;
    fn __torajs_anyv_unbox_value(v: u64) -> i64;
    /// Cross-tier — torajs-throw catchable RangeError.
    fn __torajs_throw_range_error(msg: *const u8);
}

/// The receiver as a fresh owned `Array<Any>` — a NaN-box receiver
/// slices (per-slot stakes), a typed receiver bulk-boxes through
/// the extend bridge (borrowed src, per-slot stakes taken inside).
/// Extern face: `toSpliced` seeds its copy here (the spec product
/// is a plain array — a kind-preserving copy would trip the typed
/// splice-admit on foreign items).
///
/// # Safety
/// `arr` is a valid `Tag::Arr` heap pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_any_owned_copy(arr: *const u8) -> *mut u8 {
    unsafe { owned_any_copy(arr) }
}

unsafe fn owned_any_copy(arr: *const u8) -> *mut u8 {
    unsafe {
        if (*(arr as *const HeapHeader)).flags & FLAG_ARR_ANY != 0 {
            return crate::slice::__torajs_arr_any_slice(arr, 0, i64::MAX);
        }
        let len = *(arr.add(ARR_LEN_OFF) as *const u64);
        let seed = crate::alloc::__torajs_arr_alloc_any(len);
        crate::any::__torajs_arr_extend_any(seed, arr)
    }
}

/// Does any slot of an `Array<Any>` hold another array? — the
/// flat-depth loop's early exit (an Infinity depth terminates when
/// a pass leaves nothing nested).
unsafe fn has_nested_arr(arr: *const u8) -> bool {
    unsafe {
        let len = *(arr.add(ARR_LEN_OFF) as *const u64);
        for i in 0..len {
            let av = *slot_anyvalue_ptr(arr as *mut u8, i);
            if __torajs_anyv_unbox_tag(av) as u64 != ANY_HEAP {
                continue;
            }
            let cell = __torajs_anyv_unbox_value(av) as *const u8;
            if !cell.is_null() && *(cell.add(4) as *const u16) == TAG_ARR {
                return true;
            }
        }
        false
    }
}

/// `xs.flat(depth)` for an any receiver per ES §23.1.3.13 — depth
/// arrives as ToIntegerOrInfinity (Infinity rides i64::MAX and
/// terminates through the nested scan). depth ≤ 0 answers a plain
/// kind-preserving copy; each pass reuses the depth-1
/// [`crate::any::__torajs_arr_flat_any`] kernel, intermediates drop
/// here.
///
/// # Safety
/// `arr` is a valid `Tag::Arr` heap pointer. Returned pointer is a
/// fresh owned (+1 rc) array.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_any_flat_depth(arr: *const u8, depth: i64) -> *mut u8 {
    unsafe {
        // RFC 20260810 刀 D — the flatten walk reads raw slots; loud
        // reject.
        if crate::sparse_gate::sparse_tail_rejects(
            arr as *const c_void,
            b"sparse array tail is not yet supported in Array.prototype.flat\0".as_ptr(),
        ) {
            return crate::alloc::__torajs_arr_alloc_any(0);
        }
        if depth <= 0 {
            return crate::slice::__torajs_arr_any_slice(arr, 0, i64::MAX);
        }
        let mut cur = if (*(arr as *const HeapHeader)).flags & FLAG_ARR_ANY != 0 {
            crate::any::__torajs_arr_flat_any(arr)
        } else {
            let boxed = owned_any_copy(arr);
            let out = crate::any::__torajs_arr_flat_any(boxed);
            crate::drop::__torajs_arr_drop_any(boxed as *mut c_void);
            out
        };
        let mut d = depth - 1;
        while d > 0 && has_nested_arr(cur) {
            let next = crate::any::__torajs_arr_flat_any(cur);
            crate::drop::__torajs_arr_drop_any(cur as *mut c_void);
            cur = next;
            d -= 1;
        }
        cur
    }
}

/// `xs.with(i, v)` for an any receiver per ES §23.1.3.39 — fresh
/// `Array<Any>` copy with slot `actualIndex` replaced by the
/// BORROWED NaN-box `v` (a stake is taken here). Out-of-range
/// records a catchable RangeError and answers NULL (message matches
/// the typed-tier `__torajs_arr_with`).
///
/// # Safety
/// `arr` is a valid `Tag::Arr` heap pointer; `v` is a NaN-box
/// AnyValue the caller keeps alive across the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_any_with(arr: *const u8, i: i64, v: u64) -> *mut u8 {
    unsafe {
        // RFC 20260810 刀 D — the full-copy walk reads raw slots;
        // loud reject.
        if crate::sparse_gate::sparse_tail_rejects(
            arr as *const c_void,
            b"sparse array tail is not yet supported in Array.prototype.with\0".as_ptr(),
        ) {
            return crate::alloc::__torajs_arr_alloc_any(0);
        }
        let len = *(arr.add(ARR_LEN_OFF) as *const u64) as i64;
        let adj = if i < 0 { len + i } else { i };
        if adj < 0 || adj >= len {
            __torajs_throw_range_error(b"Array index out of range\0".as_ptr());
            return core::ptr::null_mut();
        }
        let p = owned_any_copy(arr);
        let slot = slot_anyvalue_ptr(p, adj as u64);
        __torajs_value_drop_heap(*slot as *mut c_void);
        __torajs_rc_inc(v as *mut c_void);
        *slot = v;
        p
    }
}
