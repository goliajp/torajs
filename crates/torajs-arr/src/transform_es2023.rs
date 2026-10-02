//! ES2023 non-mutating `toReversed` / `with` over an 8-byte-slot
//! `Array<T>` — each builds a fresh right-sized array instead of
//! touching the receiver. Moved from [`crate::transform`] (file-size
//! split; earlier moved there from `join.rs` in chunk 624).

use core::ffi::c_void;

use crate::layout::{ARR_CAP_OFF, ARR_CELL_SIZE, ARR_DATA_PTR_OFF, ARR_LEN_OFF, TAG_ARR, arr_data};
use crate::transform::{arr_head, arr_len};

const ARR_HEAD_OFF: usize = ARR_CAP_OFF + 4;

unsafe extern "C" {
    /// torajs-mmalloc libc-compat — v0.7-A2 step 6b cutover.
    #[link_name = "__torajs_libc_malloc"]
    fn malloc(size: usize) -> *mut c_void;
}

unsafe extern "C" {
    /// Cross-tier — torajs-throw catchable RangeError (records via
    /// TLS; the caller's emit_throw_check propagates).
    fn __torajs_throw_range_error(msg: *const u8);
}

#[inline]
unsafe fn slot_addr(arr: *const u8, i: u64) -> *const u8 {
    unsafe { arr_data(arr).add((arr_head(arr) as usize + i as usize) * 8) }
}

/// Internal alloc for `Array<T>` (matches C's `arr_alloc_`).
/// Bypasses cap-matched pool — to_reversed/with always produce a fresh
/// right-sized block.
#[inline]
unsafe fn arr_alloc_fresh(len: u64, cap: u64) -> *mut u8 {
    unsafe {
        let total = ARR_CELL_SIZE + (cap as usize) * 8;
        let p = malloc(total) as *mut u8;
        *(p as *mut u32) = 1;
        *(p.add(4) as *mut u16) = TAG_ARR;
        *(p.add(6) as *mut u16) = 0;
        *(p.add(ARR_LEN_OFF) as *mut u64) = len;
        *(p.add(ARR_CAP_OFF) as *mut u32) = cap as u32;
        *(p.add(ARR_HEAD_OFF) as *mut u32) = 0;
        // props NULL (chunk 516 slice fix, same latent-garbage class)
        // + B1 self-referential data pointer.
        *(p.add(crate::layout::ARR_PROPS_OFF) as *mut u64) = 0;
        *(p.add(ARR_DATA_PTR_OFF) as *mut *mut u8) = p.add(ARR_CELL_SIZE);
        p
    }
}

// ============================================================
// arr_to_reversed — ES2023 non-mutating reverse
// ============================================================

/// `hole_fill` is the element kind's `undefined`, written where the
/// receiver has a hole (§23.1.3.33 reads each index with [[Get]]); see
/// [`crate::define_hole::fill_hole_slots`].
///
/// # Safety
/// `arr` is a live Arr cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_to_reversed(arr: *const u8, hole_fill: u64) -> *mut u8 {
    unsafe {
        let len = arr_len(arr);
        let p = arr_alloc_fresh(len, len);
        crate::layout::copy_elem_desc_bits(arr, p);
        let dst = arr_data(p);
        for i in 0..len {
            let src = slot_addr(arr, len - 1 - i);
            *(dst.add(i as usize * 8) as *mut u64) = *(src as *const u64);
        }
        if crate::layout::needs_index_mop(arr) {
            crate::define_hole::fill_hole_slots(arr, 0, p, 0, len, true, hole_fill);
        }
        p
    }
}

// ============================================================
// arr_with — ES2023 non-mutating index update
// ============================================================

/// `hole_fill` as for [`__torajs_arr_to_reversed`] (§23.1.3.39 step 10
/// reads every other index with [[Get]]).
///
/// # Safety
/// `arr` is a live Arr cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_with(
    arr: *const u8,
    i: i64,
    v: i64,
    hole_fill: u64,
) -> *mut u8 {
    unsafe {
        let len = arr_len(arr);
        // ES §23.1.3.39 step 7 — `actualIndex = i < 0 ? len + i : i`,
        // throw RangeError when out-of-range. Pre-fix this fell
        // through to the unchecked slot store and silently corrupted
        // adjacent heap (or wrote past the alloc on small caps).
        let adj = if i < 0 { len as i64 + i } else { i };
        if adj < 0 || adj >= len as i64 {
            __torajs_throw_range_error(c"Array index out of range".as_ptr().cast());
            return core::ptr::null_mut();
        }
        let p = arr_alloc_fresh(len, len);
        crate::layout::copy_elem_desc_bits(arr, p);
        let dst = arr_data(p);
        if len > 0 {
            let src = slot_addr(arr, 0);
            core::ptr::copy_nonoverlapping(src, dst, (len as usize) * 8);
        }
        if crate::layout::needs_index_mop(arr) {
            crate::define_hole::fill_hole_slots(arr, 0, p, 0, len, false, hole_fill);
        }
        *(dst.add(adj as usize * 8) as *mut u64) = v as u64;
        p
    }
}
