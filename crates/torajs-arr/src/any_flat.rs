//! `arr.flat()` depth-1 over an `Array<Any>` outer — moved verbatim
//! out of `any.rs` when the arguments-bag write pushed that file past
//! the 500-line limit.

use core::ffi::c_void;

use crate::any::{__torajs_arr_extend_any, __torajs_arr_push_any, ANY_HEAP, slot_anyvalue_ptr};
use crate::layout::{ARR_LEN_OFF, TAG_ARR};

unsafe extern "C" {
    fn __torajs_rc_inc(p: *mut c_void);
    fn __torajs_anyv_unbox_tag(v: u64) -> i64;
    fn __torajs_anyv_unbox_value(v: u64) -> i64;
}

/// `arr.flat()` depth-1 for Array<Any> outer. Each outer slot is
/// decoded — when it wraps an inner Array<Any> heap (tag = ANY_HEAP,
/// inner type_tag = TAG_ARR, inner FLAG_ARR_ANY set) the inner's
/// slots are appended via `__torajs_arr_extend_any` (which handles
/// per-cell rc_inc for shared ownership). Other slots (scalars or
/// non-Arr heap values) carry through as a single push so non-
/// arrayish elements survive flatten per ES §23.1.3.13.
///
/// v0 supports depth=1 only — matches the typed `__torajs_arr_flat`
/// contract. depth=N is unrolled at the ssa-lower layer (mirror of
/// the existing typed-flat dispatch in `ssa_lower_str.rs`).
///
/// # Safety
/// `outer` must be a valid Array<Any> heap pointer (FLAG_ARR_ANY,
/// 8-byte AnyValue slots). Inner Array<Any> pointers carried in
/// ANY_HEAP slots stay alive for the duration of the call — caller
/// holds the only reference and we walk slots before dst is filled,
/// so no drop races occur even if dst grows.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_flat_any(outer: *const u8) -> *mut u8 {
    // RFC 20260810 刀 D — the outer walk reads raw slots; loud
    // reject (a sparse INNER array is caught by `extend_any`).
    if unsafe {
        crate::sparse_gate::sparse_tail_rejects(
            outer as *const c_void,
            c"sparse array tail is not yet supported in Array.prototype.flat"
                .as_ptr()
                .cast(),
        )
    } {
        return unsafe { crate::alloc_any::__torajs_arr_alloc_any(0) };
    }
    let outer_len = unsafe { *(outer.add(ARR_LEN_OFF) as *const u64) };
    // Start with cap == outer_len; arr_push_any / arr_extend_any
    // grow on demand so over-estimating isn't required.
    let mut dst = unsafe { crate::alloc_any::__torajs_arr_alloc_any(outer_len) };
    let outer_mut = outer as *mut u8;
    let outer_mop = unsafe { crate::layout::needs_index_mop(outer) };
    for i in 0..outer_len {
        let av = unsafe { *slot_anyvalue_ptr(outer_mut, i) };
        // §23.1.3.13.1 FlattenIntoArray step 3.b — a hole is skipped:
        // a slot hole reads `0`, a shadow hole only answers the funnel.
        if av == 0
            || (outer_mop
                && unsafe {
                    crate::define_hole::__torajs_arr_has_index(outer_mut.cast(), i as i64)
                } == 0)
        {
            continue;
        }
        let tag = unsafe { __torajs_anyv_unbox_tag(av) } as u64;
        let value = unsafe { __torajs_anyv_unbox_value(av) };
        if tag == ANY_HEAP {
            let inner = value as *const u8;
            if !inner.is_null() {
                let inner_type_tag = unsafe { *(inner.add(4) as *const u16) };
                // Chunk 624 — typed inner arrays flatten too:
                // extend_any's src dispatch bulk-boxes them per
                // their recorded elem kind (pre-fix they carried
                // through as a single scalar-looking element).
                if inner_type_tag == TAG_ARR {
                    dst = if unsafe { crate::layout::needs_index_mop(inner) } {
                        unsafe { crate::flat_present::extend_present(dst, inner) }
                    } else {
                        unsafe { __torajs_arr_extend_any(dst, inner) }
                    };
                    continue;
                }
            }
        }
        // Non-array slot — preserve as one element. push_any takes
        // ownership of ANY_HEAP cells, so bump the refcount before
        // handing it over (mirrors arr_extend_any's per-slot
        // rc_inc loop).
        unsafe { __torajs_rc_inc(av as *mut c_void) };
        dst = unsafe { __torajs_arr_push_any(dst as *mut c_void, tag, value as u64) };
    }
    dst
}
