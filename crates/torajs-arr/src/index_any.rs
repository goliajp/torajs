//! `__torajs_arr_index_get` — kind-aware indexed element read for an
//! array reached through an `any` receiver.
//!
//! Any-dynamic-access RFC (20260704) S3. The dispatch entry
//! (`torajs-anyvalue::index_any::__torajs_any_index_get`) unboxes the
//! receiver and routes `Tag::Arr` cells here; this fn owns the array
//! layout knowledge:
//!
//! - `FLAG_ARR_ANY` — slots are NaN-box AnyValues; read directly.
//! - typed kinds (written by [`crate::mark_kind`] at the boxing
//!   boundary) — re-box the raw slot per `ARR_KIND_*`.
//! - `ARR_KIND_UNSET` — the array never crossed a marking boundary
//!   (a missed boxing site, or a > 21-level nesting chain tail);
//!   debug builds assert, release returns `undefined`.
//!
//! Out-of-bounds / negative index → `undefined` (ES §10.4.2.1), same
//! contract as `__torajs_arr_get_any_*`. Returned cell values carry
//! a +1 refcount (the slot keeps its own reference) — the SSA layer
//! releases the returned Any via the usual `anyv_rc_dec` drop.

use core::ffi::c_void;

use torajs_rc::{
    ARR_KIND_BOOL, ARR_KIND_F64, ARR_KIND_HEAP, ARR_KIND_I64, ARR_KIND_UNSET, FLAG_ARR_ANY,
    HeapHeader,
};

use crate::layout::{ARR_LEN_OFF, ARR_SLOTS_OFF};

const ARR_HEAD_OFF: usize = 20;

unsafe extern "C" {
    /// Cross-tier — torajs-anyvalue NaN-box pack. Tag scheme:
    /// 0=Null, 1=Bool, 2=I64, 3=F64 (bits), 4=Heap, 5=Undef.
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
    /// Cross-tier — torajs-rc. NaN-box-safe refcount bump (no-ops
    /// for non-cell bit patterns and NULL).
    fn __torajs_rc_inc(p: *mut c_void);
}

/// NaN-box `undefined` sentinel via the pair packer (tag 5).
#[inline]
unsafe fn undef() -> u64 {
    unsafe { __torajs_anyv_box_from_pair(5, 0) }
}

/// Kind-aware `arr[idx]` read. See module doc for the contract.
///
/// # Safety
/// `arr` is either NULL or a valid `Tag::Arr` heap pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_index_get(arr: *const c_void, idx: i64) -> u64 {
    unsafe {
        if arr.is_null() || idx < 0 {
            return undef();
        }
        let p = arr as *const u8;
        let len = *(p.add(ARR_LEN_OFF) as *const u64);
        if idx as u64 >= len {
            return undef();
        }
        let head = *(p.add(ARR_HEAD_OFF) as *const u32) as u64;
        let slot = p.add(ARR_SLOTS_OFF + ((head + idx as u64) as usize) * 8);
        let raw = *(slot as *const u64);
        let header = &*(arr as *const HeapHeader);
        if header.flags & FLAG_ARR_ANY != 0 {
            // NaN-box slot — the slot keeps its own reference, the
            // returned copy takes another (+1 for cells, no-op for
            // immediates).
            __torajs_rc_inc(raw as *mut c_void);
            return raw;
        }
        match header.arr_elem_kind() {
            ARR_KIND_I64 => __torajs_anyv_box_from_pair(2, raw as i64),
            ARR_KIND_F64 => __torajs_anyv_box_from_pair(3, raw as i64),
            ARR_KIND_BOOL => __torajs_anyv_box_from_pair(1, raw as i64),
            ARR_KIND_HEAP => {
                __torajs_rc_inc(raw as *mut c_void);
                __torajs_anyv_box_from_pair(4, raw as i64)
            }
            kind => {
                debug_assert!(
                    kind == ARR_KIND_UNSET,
                    "arr_index_get: invalid elem kind {kind}"
                );
                debug_assert!(
                    false,
                    "arr_index_get: UNSET elem kind — a typed-arr→Any \
                     boxing site missed __torajs_arr_mark_kind"
                );
                undef()
            }
        }
    }
}
