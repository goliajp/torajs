//! RFC 20260721-typed-grow-on-write — typed-block index writes past
//! the end grow-as-holes (split out of `grow.rs`, file-size hard
//! limit).
//!
//! `a[i] = v` on a typed block with `i >= len` grows per ES
//! §10.4.2.1 OrdinarySet on an array index: the `[len, i)` gap
//! zero-fills and HOLE-marks (undefined-reading slots that are not
//! own properties — same shadow representation the `arr.length = N`
//! grow of 刀 5 G3 ships), `i` itself becomes a real own property,
//! `len = i + 1`. The static typed lane calls the
//! [`__torajs_arr_typed_set_grow`] kernel from its OOB arm; the
//! `any[]`-view lane reaches [`typed_grow_store`] through
//! `any_typed_bridge::typed_set_grow` after its kind coercion.

use core::ffi::c_void;

use crate::grow::grow_data_buffer;
use crate::layout::{ARR_CAP_OFF, ARR_LEN_OFF, arr_data};

/// Offset of the `head_offset` u32 (T-13.5 deque packed cap + head).
const ARR_HEAD_OFF: usize = ARR_CAP_OFF + 4;

unsafe extern "C" {
    /// Cross-tier — torajs-throw catchable RangeError (returns
    /// normally; the caller's `emit_throw_check` propagates).
    fn __torajs_throw_range_error(msg: *const u8);

    /// Cross-tier — universal heap value dropper (NaN-box safe; the
    /// cell gate skips immediates).
    fn __torajs_value_drop_heap(p: *mut c_void);
}

/// Grow a typed block to hold a store at index `i >= len`: compact
/// the deque head, grow the data buffer (2× amortized, matching
/// `__torajs_arr_set_any_grow`, so an index-append loop stays O(1)
/// amortized), zero-fill the `[len, i)` gap, store `raw` at `i`, set
/// `len = i + 1`, and mark the gap indexes HOLES (§10.4.2.1 — the
/// grown slots are not own properties; `i` itself IS, so it stays
/// unmarked and a plain append pays no shadow cost).
pub(crate) unsafe fn typed_grow_store(arr: *mut u8, i: u64, raw: u64) {
    unsafe {
        let old = *(arr.add(ARR_LEN_OFF) as *const u64);
        let head = *(arr.add(ARR_HEAD_OFF) as *const u32) as usize;
        if head > 0 {
            let data = arr_data(arr);
            core::ptr::copy(data.add(head * 8), data, old as usize * 8);
            *(arr.add(ARR_HEAD_OFF) as *mut u32) = 0;
        }
        let cap = *(arr.add(ARR_CAP_OFF) as *const u32) as u64;
        if cap < i + 1 {
            let mut new_cap: u64 = if cap == 0 { 4 } else { cap * 2 };
            if new_cap < i + 1 {
                new_cap = i + 1;
            }
            grow_data_buffer(arr, new_cap);
        }
        let slots = arr_data(arr);
        for k in old..i {
            *(slots.add(k as usize * 8) as *mut u64) = 0;
        }
        *(slots.add(i as usize * 8) as *mut u64) = raw;
        *(arr.add(ARR_LEN_OFF) as *mut u64) = i + 1;
        crate::define_hole::mark_hole_range(arr as *mut c_void, old, i);
    }
}

/// The typed-tier `arr[i] = v` OOB arm (replaces the bug-327 C3 loud
/// rejector): grow-as-holes + store, the typed twin of
/// `__torajs_arr_set_any_grow`'s grow path. `v` is the raw 8-byte
/// slot value (f64 bit-punned, heap cell transferred — the slot
/// takes the caller's +1; a reject arm drops it for HEAP-kind
/// elements so the stake never leaks).
///
/// - `i < 0` — RangeError (a negative index is a string-keyed
///   property per ES; that lane is a recorded boundary).
/// - `i >= ARR_DENSE_LIMIT` — RangeError, same sparse-storage
///   message as the Array<Any> lane (one red family for the G4 RFC).
///
/// # Safety
/// `arr` is a valid non-Any typed array heap pointer; caller checks
/// for a pending throw after return.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_typed_set_grow(arr: *mut u8, i: i64, v: i64) {
    unsafe {
        let is_heap_kind =
            (*(arr as *const torajs_rc::HeapHeader)).arr_elem_kind() == torajs_rc::ARR_KIND_HEAP;
        if i < 0 {
            if is_heap_kind {
                __torajs_value_drop_heap(v as *mut c_void);
            }
            __torajs_throw_range_error(
                b"negative array index write (string-keyed element properties are not yet supported)\0"
                    .as_ptr(),
            );
            return;
        }
        if i as u64 >= crate::any::ARR_DENSE_LIMIT {
            if is_heap_kind {
                __torajs_value_drop_heap(v as *mut c_void);
            }
            __torajs_throw_range_error(
                b"array index beyond the dense-storage limit (sparse arrays are not yet supported)\0"
                    .as_ptr(),
            );
            return;
        }
        typed_grow_store(arr, i as u64, v as u64);
    }
}
