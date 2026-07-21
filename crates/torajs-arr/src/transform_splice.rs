//! `Array.prototype.splice` kernels (ES §23.1.3.31) — the shrink-only
//! form moved verbatim from [`crate::transform`] plus the `...items`
//! insert sibling (RFC 20260720-splice-insert knife 1).
//!
//! Both operate on 8-byte-slot `Array<T>` receivers; the `Array<Any>`
//! NaN-box lane has its own full-featured kernel in
//! [`crate::method_any_transform::__torajs_arr_any_splice`], which
//! this insert kernel mirrors structurally (head fold → removed
//! transfer → gap resize → item store). The differences are the item
//! encoding (raw elem-repr slots, already coerced by the SSA layer)
//! and the rc contract (zero rc traffic here — the SSA emit site owns
//! inc/hand-off, same as the push lane).

use crate::layout::arr_data;
use crate::transform::{
    arr_alloc_with, arr_cap, arr_head, arr_len, data_ptr, data_ptr_raw, set_arr_head, set_arr_len,
};

/// `arr.splice(start, delete_count)` — remove `delete_count` slots
/// starting at logical `start`, returning the removed slice as a
/// fresh `Array<T>`. Trailing slots compact left into the gap; the
/// receiver's `len` shrinks by the actual delete count. Subset:
/// no `...items` insert args (the insert form is the dedicated
/// [`__torajs_arr_splice_items`] below — this 3-arg fast path stays
/// memmove-only).
///
/// Per ES spec §23.1.3.31:
///   - `start < 0`        → `max(len + start, 0)`
///   - `start > len`      → `len`
///   - `delete_count < 0` → `0`
///   - `delete_count > len - actual_start` → `len - actual_start`
///
/// Receiver pointer is unchanged (no realloc — splice only shrinks
/// the live range), so the SSA dispatch can skip the slot-writeback
/// that push / unshift need.
///
/// # Safety
/// `arr` must be a valid Array<T> heap block (8-byte slots).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_splice(
    arr: *mut u8,
    start: i64,
    delete_count: i64,
) -> *mut u8 {
    // 刀 6 G9a — a FROZEN receiver throws before any mutation (the
    // spec's first element Set/Delete rejects); a merely
    // length-locked one mutates first and throws at the step-24
    // length write ([`splice_finish_len`]).
    if let Some(removed) = unsafe { splice_frozen_reject(arr) } {
        return removed;
    }
    let len = unsafe { arr_len(arr) } as i64;
    let (actual_start, actual_delete) = normalize_splice_range(start, delete_count, len);
    let removed = unsafe { arr_alloc_with(actual_delete as u64, actual_delete as u64) };
    unsafe { crate::layout::copy_elem_desc_bits(arr, removed) };
    if actual_delete > 0 {
        unsafe {
            core::ptr::copy_nonoverlapping(
                data_ptr(arr).add(actual_start as usize * 8),
                data_ptr_raw(removed, 0),
                actual_delete as usize * 8,
            );
        }
    }
    let trailing = len - actual_start - actual_delete;
    if trailing > 0 && actual_delete > 0 {
        unsafe {
            core::ptr::copy(
                data_ptr(arr).add((actual_start + actual_delete) as usize * 8),
                data_ptr(arr).add(actual_start as usize * 8),
                trailing as usize * 8,
            );
        }
    }
    unsafe {
        splice_finish_len(arr, len as u64, (len - actual_delete) as u64);
    }
    removed
}

/// 刀 6 G9a — the FROZEN entry reject shared by the splice kernels:
/// `Some(empty removed)` with the TypeError recorded, `None` to
/// proceed. Frozen elements reject the spec's first element move, so
/// nothing may mutate; the length-lock-only shape instead falls to
/// [`splice_finish_len`]'s post-mutation throw.
pub(crate) unsafe fn splice_frozen_reject(arr: *mut u8) -> Option<*mut u8> {
    let flags = unsafe { ((arr as *const u8).add(6) as *const u16).read() };
    if flags & torajs_rc::FLAG_FROZEN == 0 {
        return None;
    }
    unsafe { crate::define_length::__torajs_arr_len_write_guard(arr as *const _) };
    let removed = unsafe { arr_alloc_with(0, 0) };
    unsafe { crate::layout::copy_elem_desc_bits(arr, removed) };
    Some(removed)
}

/// §23.1.3.31 step 24 — the final length write, shared by the three
/// splice kernels. A locked length throws AFTER the element moves
/// (spec order — the trailing deletes have already landed), so the
/// range `[new_len, old_len)` becomes holes over the stale
/// moved-left duplicates (overwrite only, their references
/// transferred left) and `len` keeps its old value. Net-GROWTH
/// callers guard at entry instead: §10.4.2.1 step 2.c rejects the
/// first element write past the old length before any mutation.
pub(crate) unsafe fn splice_finish_len(arr: *mut u8, old_len: u64, new_len: u64) {
    unsafe {
        if crate::define_length::__torajs_arr_len_write_guard(arr as *const _) == 0 {
            set_arr_len(arr, new_len);
            return;
        }
        debug_assert!(new_len <= old_len, "growth callers guard at entry");
        let is_any =
            ((arr as *const u8).add(6) as *const u16).read() & torajs_rc::FLAG_ARR_ANY != 0;
        let fill: u64 = if is_any { 0x0A } else { 0 };
        for k in new_len..old_len {
            *(data_ptr(arr).add(k as usize * 8) as *mut u64) = fill;
        }
        crate::define_hole::mark_hole_range(arr as *mut core::ffi::c_void, new_len, old_len);
    }
}

/// §23.1.3.31 steps 5-7 — clamp `start` / `delete_count` to the
/// actual range. Shared by both splice kernels so the insert form
/// can't drift from the shrink form's spec math.
fn normalize_splice_range(start: i64, delete_count: i64, len: i64) -> (i64, i64) {
    let actual_start = if start < 0 {
        let s = start + len;
        if s < 0 { 0 } else { s }
    } else if start > len {
        len
    } else {
        start
    };
    let actual_delete = if delete_count < 0 {
        0
    } else if delete_count > len - actual_start {
        len - actual_start
    } else {
        delete_count
    };
    (actual_start, actual_delete)
}

/// `arr.splice(start, delete_count, ...items)` — the insert form:
/// remove `actual_delete` slots at `actual_start`, then store the
/// `n_items` raw slots in their place, growing the data buffer when
/// the insert outsizes the delete. Returns the removed slice as a
/// fresh `Array<T>` (elem-desc flags copied from the receiver, so an
/// `Array<Any>` receiver's removed product keeps its NaN-box shape).
///
/// Item slots are RAW elem-repr bits — the SSA emit site has already
/// coerced each item (`coerce_push_value`) and owns the rc ledger
/// (borrowed values inc'd, owned mints handed off), so this kernel
/// moves bits only. Removed slots likewise TRANSFER their references
/// into the returned array (net-zero rc, same as the shrink form).
///
/// The receiver cell never moves (B1 — grow swaps the data buffer),
/// so no write-back slot is needed. A non-zero deque `head_offset`
/// is folded to 0 first so the gap math runs on physical slot 0
/// (mirrors the any-lane kernel).
///
/// # Safety
/// `arr` must be a valid Array<T> heap block (8-byte slots);
/// `items` must point at `n_items` readable i64 slots.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_splice_items(
    arr: *mut u8,
    start: i64,
    delete_count: i64,
    items: *const i64,
    n_items: i64,
) -> *mut u8 {
    // 刀 6 G9a — frozen rejects at entry; a net-growth shape with a
    // locked length rejects before mutation too (§10.4.2.1 step 2.c:
    // the first element write past the old length needs the implicit
    // length bump a read-only length refuses). Items are raw
    // transferred bits; the throw paths leave them to the emit
    // site's ledger, same as the any-lane kernel's TypeError
    // rejections.
    if let Some(removed) = unsafe { splice_frozen_reject(arr) } {
        return removed;
    }
    let len = unsafe { arr_len(arr) } as i64;
    let (actual_start, actual_delete) = normalize_splice_range(start, delete_count, len);
    if n_items > actual_delete
        && unsafe { crate::define_length::__torajs_arr_len_write_guard(arr as *const _) } != 0
    {
        let removed = unsafe { arr_alloc_with(0, 0) };
        unsafe { crate::layout::copy_elem_desc_bits(arr, removed) };
        return removed;
    }

    // Fold a deque head down to 0 so the gap math below works on
    // physical slot 0 (typed receivers can have shifted).
    let head = unsafe { arr_head(arr) };
    if head > 0 {
        unsafe {
            core::ptr::copy(
                arr_data(arr).add(head as usize * 8),
                arr_data(arr),
                len as usize * 8,
            );
            set_arr_head(arr, 0);
        }
    }

    let removed = unsafe { arr_alloc_with(actual_delete as u64, actual_delete as u64) };
    unsafe { crate::layout::copy_elem_desc_bits(arr, removed) };
    if actual_delete > 0 {
        unsafe {
            core::ptr::copy_nonoverlapping(
                arr_data(arr).add(actual_start as usize * 8),
                data_ptr_raw(removed, 0),
                actual_delete as usize * 8,
            );
        }
    }

    let diff = n_items - actual_delete;
    let new_len = len + diff;
    if diff > 0 {
        let cap = unsafe { arr_cap(arr) } as i64;
        if new_len > cap {
            let grown = if cap * 2 > new_len { cap * 2 } else { new_len };
            unsafe { crate::grow::grow_data_buffer(arr, grown as u64) };
        }
    }
    let trailing = len - actual_start - actual_delete;
    if trailing > 0 && diff != 0 {
        unsafe {
            core::ptr::copy(
                arr_data(arr).add((actual_start + actual_delete) as usize * 8),
                arr_data(arr).add((actual_start + n_items) as usize * 8),
                trailing as usize * 8,
            );
        }
    }
    for k in 0..n_items {
        unsafe {
            let slot = arr_data(arr).add((actual_start + k) as usize * 8) as *mut i64;
            *slot = *items.add(k as usize);
        }
    }
    unsafe { splice_finish_len(arr, len as u64, new_len as u64) };
    removed
}

#[cfg(test)]
mod tests {
    use super::*;

    unsafe fn slots(arr: *const u8, len: usize) -> Vec<i64> {
        (0..len)
            .map(|i| unsafe { (data_ptr(arr).add(i * 8) as *const i64).read() })
            .collect()
    }

    fn make_arr(vals: &[i64]) -> *mut u8 {
        unsafe {
            let p = arr_alloc_with(vals.len() as u64, vals.len() as u64);
            for (i, v) in vals.iter().enumerate() {
                (data_ptr_raw(p, i) as *mut i64).write(*v);
            }
            p
        }
    }

    #[test]
    fn insert_replaces_one_with_one() {
        let arr = make_arr(&[1, 2, 3]);
        let items = [9i64];
        let removed = unsafe { __torajs_arr_splice_items(arr, 1, 1, items.as_ptr(), 1) };
        unsafe {
            assert_eq!(arr_len(arr), 3);
            assert_eq!(slots(arr, 3), vec![1, 9, 3]);
            assert_eq!(arr_len(removed), 1);
            assert_eq!(slots(removed, 1), vec![2]);
        }
    }

    #[test]
    fn insert_grows_past_cap() {
        let arr = make_arr(&[1, 2]);
        let items = [7i64, 8, 9];
        let removed = unsafe { __torajs_arr_splice_items(arr, 1, 0, items.as_ptr(), 3) };
        unsafe {
            assert_eq!(arr_len(arr), 5);
            assert_eq!(slots(arr, 5), vec![1, 7, 8, 9, 2]);
            assert_eq!(arr_len(removed), 0);
        }
    }

    #[test]
    fn insert_at_tail_is_append() {
        let arr = make_arr(&[1, 2]);
        let items = [5i64];
        unsafe {
            __torajs_arr_splice_items(arr, 2, 0, items.as_ptr(), 1);
            assert_eq!(arr_len(arr), 3);
            assert_eq!(slots(arr, 3), vec![1, 2, 5]);
        }
    }

    #[test]
    fn delete_more_than_insert_compacts() {
        let arr = make_arr(&[1, 2, 3, 4, 5]);
        let items = [9i64];
        let removed = unsafe { __torajs_arr_splice_items(arr, 1, 3, items.as_ptr(), 1) };
        unsafe {
            assert_eq!(arr_len(arr), 3);
            assert_eq!(slots(arr, 3), vec![1, 9, 5]);
            assert_eq!(slots(removed, 3), vec![2, 3, 4]);
        }
    }

    #[test]
    fn head_folded_deque_receiver() {
        // Simulate a shifted deque: head=1 over [_, 2, 3] (logical
        // [2, 3]) — insert must fold head first.
        let arr = make_arr(&[1, 2, 3]);
        unsafe {
            set_arr_head(arr, 1);
            set_arr_len(arr, 2);
            let items = [9i64];
            __torajs_arr_splice_items(arr, 1, 0, items.as_ptr(), 1);
            assert_eq!(arr_head(arr), 0);
            assert_eq!(arr_len(arr), 3);
            assert_eq!(slots(arr, 3), vec![2, 9, 3]);
        }
    }

    #[test]
    fn negative_start_clamps() {
        let arr = make_arr(&[1, 2, 3]);
        let items = [9i64];
        unsafe {
            __torajs_arr_splice_items(arr, -1, 1, items.as_ptr(), 1);
            assert_eq!(arr_len(arr), 3);
            assert_eq!(slots(arr, 3), vec![1, 2, 9]);
        }
    }
}
