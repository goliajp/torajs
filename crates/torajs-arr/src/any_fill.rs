//! `Array.prototype.fill` over `Array<Any>` — split out of `any.rs`
//! (file-size hard limit; RFC 20260713-defprop-residual-cluster
//! chunk C pushed the shared file over 500).

use core::ffi::c_void;

use torajs_rc::{FLAG_ARR_ANY, HeapHeader};

use crate::any::slot_anyvalue_ptr;
use crate::layout::ARR_LEN_OFF;

unsafe extern "C" {
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
    fn __torajs_rc_inc(p: *mut c_void);
    fn __torajs_value_drop_heap(p: *mut c_void);
}

/// `arr.fill((tag, value), start, end)` for Array<Any> — write the
/// NaN-boxed AnyValue into each slot in `[start, end)`. Indices are
/// clamped to `[0, len]`. Each pre-existing slot is dropped first
/// (value_drop_heap is NaN-box-safe, no-ops on primitives) so a
/// fill over heap-tagged ANY_HEAP slots doesn't leak; the fill
/// value is rc_inc'd per replaced slot when it's an ANY_HEAP cell
/// so each slot owns a balanced ref. Mirrors the typed
/// `__torajs_arr_fill` contract (returns the same pointer; never
/// reallocs).
///
/// # Safety
/// `arr` must be a valid Array<Any> heap block (FLAG_ARR_ANY,
/// 8-byte AnyValue slot stride).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_fill_any(
    arr: *mut c_void,
    tag: u64,
    value: u64,
    start: i64,
    end: i64,
) -> *mut u8 {
    let arr = arr as *mut u8;
    unsafe {
        // RFC 20260810 刀 D — the fill walk crosses the
        // unmaterialized tail; loud reject.
        if crate::sparse_gate::sparse_tail_rejects(
            arr as *const c_void,
            b"sparse array tail is not yet supported in Array.prototype.fill\0".as_ptr(),
        ) {
            return arr;
        }
        let len = *(arr.add(ARR_LEN_OFF) as *const u64) as i64;
        let lo = if start < 0 {
            0
        } else if start > len {
            len
        } else {
            start
        };
        let hi = if end < 0 {
            0
        } else if end > len {
            len
        } else {
            end
        };
        if hi <= lo {
            return arr;
        }
        if (*(arr as *const HeapHeader)).flags & FLAG_ARR_ANY == 0 {
            // Chunk 622 — typed block behind the static Arr<Any>
            // view: raw-fill per the elem kind (mismatch TypeError).
            crate::any_typed_bridge::typed_fill_pair(arr, tag, value, lo, hi);
            return arr;
        }
        let av = __torajs_anyv_box_from_pair(tag as i64, value as i64);
        for i in lo..hi {
            let slot = slot_anyvalue_ptr(arr, i as u64);
            let old_av = *slot;
            __torajs_value_drop_heap(old_av as *mut c_void);
            *slot = av;
            // rc_inc is NaN-box-safe — no-op for primitives, bumps
            // the wrapped heap pointer's refcount for ANY_HEAP cells.
            __torajs_rc_inc(av as *mut c_void);
        }
        // 刀 13d follow-up (RFC 20260721-array-proto-cluster) — the
        // §23.1.3.7 write is a Set: a filled hole becomes a live
        // default data property again. Pre-fix the stale F_HOLE
        // shadow made every hole-aware consumer (`in` /
        // hasOwnProperty / [[Get]]) treat the freshly written index
        // as still absent (test262
        // fill/fill-values-custom-start-and-end).
        if (*(arr as *const HeapHeader)).flags & torajs_rc::FLAG_ARR_EXOTIC_INDEX != 0 {
            for i in lo..hi {
                crate::define_hole::revive_index_if_hole(arr as *mut c_void, i as u64);
            }
        }
        arr
    }
}
