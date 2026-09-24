//! §23.1.3.13.1 FlattenIntoArray over an inner array whose indices are
//! not all plain own data properties — split from `any.rs` (file-size
//! limit), which keeps the raw-slot bulk copy for the common case.
//!
//! Step 3.b asks HasProperty for every index before it reads one, so a
//! hole (the slot value `0` of a `FLAG_ARR_ANY_HOLEY` cell, or a shadow
//! entry) is skipped rather than copied as `undefined`, an accessor
//! index runs its getter, and a hole that `Array.prototype` supplies
//! reads through to it. `__torajs_arr_extend_any` is a Get-shaped copy
//! (spread rides it), so it cannot answer that question.

use core::ffi::c_void;

use crate::layout::ARR_LEN_OFF;

unsafe extern "C" {
    /// NaN-box-safe release of an owned AnyValue.
    fn __torajs_value_drop_heap(p: *mut c_void);
}

/// Append every PRESENT index of `inner` to `dst` (depth-1 flatten of
/// one element). Answers the possibly-grown `dst`.
///
/// # Safety
/// `dst` is a live `FLAG_ARR_ANY` array cell; `inner` a live array
/// cell (any element kind) that `dst` does not alias.
pub(crate) unsafe fn extend_present(dst: *mut u8, inner: *const u8) -> *mut u8 {
    unsafe {
        let mut dst = dst;
        let len = *(inner.add(ARR_LEN_OFF) as *const u64);
        for i in 0..len {
            if crate::define_hole::__torajs_arr_has_index(inner as *mut c_void, i as i64) == 0 {
                continue;
            }
            let v = crate::index_any::__torajs_arr_index_get(inner as *const c_void, i as i64);
            dst = crate::any::__torajs_arr_push_any_boxed(dst, v);
            __torajs_value_drop_heap(v as *mut c_void);
        }
        dst
    }
}

/// `flat()` at a layer whose elements are not arrays, over a typed
/// source: the Get-shaped bulk copy, then its holes left out — step 3.b
/// asks HasProperty for every index, and a typed slot's word does not
/// say absent (the shadow does). A dense source is the plain copy.
///
/// Recorded boundary: a hole a prototype digit key fills is present to
/// HasProperty and the spec copies that key's value; this drops it.
///
/// # Safety
/// `arr` is a live typed array cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_flat_present(arr: *const u8) -> *mut u8 {
    unsafe {
        let len = *(arr.add(ARR_LEN_OFF) as *const u64);
        let p = crate::slice::__torajs_arr_slice(arr, 0, len as i64);
        if !crate::layout::needs_index_mop(arr) {
            return p;
        }
        let slots = crate::layout::arr_data(p) as *mut u64;
        let mut w = 0usize;
        for i in 0..len {
            let flags =
                crate::define_index_flags::__torajs_arr_index_flags(arr as *const c_void, i);
            if flags & crate::define::F_HOLE == 0 {
                *slots.add(w) = *slots.add(i as usize);
                w += 1;
            }
        }
        *(p.add(ARR_LEN_OFF) as *mut u64) = w as u64;
        p
    }
}
