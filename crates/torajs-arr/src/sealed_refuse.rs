//! The SEALED receiver's refusals for the two shapes that delete a
//! tail element after moving the ones before it — `shift` and a
//! shrinking `splice`.
//!
//! §23.1.3.29 shift and §23.1.3.31 splice both Set the surviving
//! elements one slot down FIRST and only then DeletePropertyOrThrow
//! the last index. A seal leaves elements writable but not
//! configurable, so the moves succeed and the delete fails: node and
//! bun answer `Object.seal([3,1,2]).shift()` with a TypeError and the
//! array left as `[1,2,2]`, length 3. A frozen receiver never gets
//! this far (the length guard refuses at entry), and a merely
//! non-extensible one is legal. So the refusal here is not "throw
//! before touching anything" — it is the spec's partial mutation,
//! reproduced: the slots past the new length keep the values the move
//! left behind, and since those are now a second visible reference to
//! a cell that has one stake, each one takes its own +1.

use core::ffi::c_void;

use torajs_rc::{
    __torajs_rc_inc, ARR_KIND_HEAP, FLAG_ARR_ANY, FLAG_ARR_LENGTH_RO, FLAG_FROZEN, FLAG_SEALED,
    HeapHeader,
};

use crate::define_length::__torajs_arr_len_write_guard;

use crate::layout::{ARR_LEN_OFF, arr_data};

const ARR_HEAD_OFF: usize = 20;

unsafe extern "C" {
    fn __torajs_value_drop_heap(p: *mut c_void);
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

/// Take a +1 on every slot in `[from, to)` that holds a cell, on either
/// block layout — the AnyValue lane through the NaN-box-safe probe, the
/// raw HEAP lane directly. Scalar lanes hold nothing to stake.
unsafe fn stake_visible_copies(arr: *mut u8, from: u64, to: u64) {
    let header = unsafe { &*(arr as *const HeapHeader) };
    let head = unsafe { *(arr.add(ARR_HEAD_OFF) as *const u32) } as usize;
    let is_any = header.flags & FLAG_ARR_ANY != 0;
    let is_heap = !is_any && header.arr_elem_kind() == ARR_KIND_HEAP;
    if !is_any && !is_heap {
        return;
    }
    for k in from..to {
        let slot = unsafe { *(arr_data(arr).add((head + k as usize) * 8) as *const u64) };
        if is_any {
            if torajs_rc::ffi::nan_box_is_cell_like(slot as *mut c_void) {
                unsafe { __torajs_rc_inc(slot as *mut c_void) };
            }
        } else if slot != 0 {
            unsafe { __torajs_rc_inc(slot as *mut c_void) };
        }
    }
}

/// `shift` on a SEALED receiver with at least one element: drop slot
/// 0's value (the `first` the spec read and then abandoned), move the
/// rest one slot down, stake the copy the move left in the last slot,
/// record the TypeError. Answers `true` when it did all that — the
/// caller stores nothing and answers the throw sentinel. Callers
/// guarantee `len > 0` and that the frozen / RO-length guard has
/// already run (a frozen receiver refuses before reaching here).
///
/// # Safety
/// `arr` is a live array heap block with `len > 0`.
pub(crate) unsafe fn shift_refuses_on_sealed(arr: *mut u8) -> bool {
    let header = unsafe { &*(arr as *const HeapHeader) };
    if header.flags & FLAG_SEALED == 0 {
        return false;
    }
    let len = unsafe { *(arr.add(ARR_LEN_OFF) as *const u64) };
    let head = unsafe { *(arr.add(ARR_HEAD_OFF) as *const u32) } as usize;
    let base = unsafe { arr_data(arr).add(head * 8) };
    let is_any = header.flags & FLAG_ARR_ANY != 0;
    let is_heap = !is_any && header.arr_elem_kind() == ARR_KIND_HEAP;
    let slot0 = unsafe { *(base as *const u64) };
    if is_any || (is_heap && slot0 != 0) {
        unsafe { __torajs_value_drop_heap(slot0 as *mut c_void) };
    }
    if len > 1 {
        unsafe { core::ptr::copy(base.add(8), base, (len - 1) as usize * 8) };
    }
    unsafe { stake_visible_copies(arr, len - 1, len) };
    unsafe { __torajs_throw_type_error(c"Unable to delete property.".as_ptr()) };
    true
}

/// The typed lowering's lock raiser for `shift`, called from the cold
/// block `emit_arr_len_lock_guard(ShiftDown)` opens when the header
/// carries any of `FROZEN | LENGTH_RO | SEALED` — the hot path is the
/// inline mask on the header word it already loads, so a plain
/// receiver pays nothing for this. The any lane's `shift` asks the
/// same question. §23.1.3.29 in order: an empty array only writes
/// `length` (step 3.b, the same value), which a non-writable length
/// refuses and a seal does not; with one element nothing moves and
/// step 6's delete of that element fails first on a frozen or sealed
/// receiver; with more, the first move's Set fails on a frozen one
/// (readonly), while a sealed one takes the partial move above and an
/// RO-length one refuses at entry. Non-zero means a TypeError is
/// pending.
///
/// # Safety
/// `arr` is a live array heap block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_shift_guard(arr: *const c_void) -> i64 {
    let arr = arr as *mut u8;
    let header = unsafe { &*(arr as *const HeapHeader) };
    let len = unsafe { *(arr.add(ARR_LEN_OFF) as *const u64) };
    if len == 0 {
        if header.flags & (FLAG_FROZEN | FLAG_ARR_LENGTH_RO) == 0 {
            return 0;
        }
        return unsafe { __torajs_arr_len_write_guard(arr as *const c_void) };
    }
    if header.flags & FLAG_FROZEN != 0 && len == 1 {
        unsafe { __torajs_throw_type_error(c"Unable to delete property.".as_ptr()) };
        return 1;
    }
    if header.flags & (FLAG_FROZEN | FLAG_ARR_LENGTH_RO) != 0 {
        return unsafe { __torajs_arr_len_write_guard(arr as *const c_void) };
    }
    unsafe { shift_refuses_on_sealed(arr) as i64 }
}

/// A shrinking `splice` on a SEALED receiver, called by the splice
/// kernels AFTER the surviving elements have been moved down and
/// BEFORE any insertion (§23.1.3.31 step 15 fails before step 17):
/// stake the copies left in `[new_len, old_len)`, record the
/// TypeError, answer `true`. The caller skips the insert and the
/// length write and returns its removed slice as it would on any
/// other throw path.
///
/// # Safety
/// `arr` is a live array heap block; `new_len <= old_len == its len`.
pub(crate) unsafe fn splice_shrink_refuses_on_sealed(
    arr: *mut u8,
    old_len: u64,
    new_len: u64,
) -> bool {
    if new_len >= old_len {
        return false;
    }
    let header = unsafe { &*(arr as *const HeapHeader) };
    if header.flags & FLAG_SEALED == 0 {
        return false;
    }
    unsafe { stake_visible_copies(arr, new_len, old_len) };
    unsafe { __torajs_throw_type_error(c"Unable to delete property.".as_ptr()) };
    true
}
