//! `Array.prototype.sort` over a TYPED cell that carries holes —
//! §23.1.3.30 SortIndexedProperties with holes skipped.
//!
//! A typed cell's hole is a shadow mark over a slot word (the F64
//! undefined sentinel, a NULL pointer); the fast sorts permute slot
//! words and leave the marks where they were, so a present element
//! ended up hidden under a hole. The spec collects the present
//! indexes, sorts them (undefined after every value, never passed to
//! the comparator), writes them back from index 0 and deletes the
//! rest — so the holes all end up at the tail.
//!
//! This kernel does exactly that on the raw slots: take the marks,
//! sort a copy of the present words through the any-receiver
//! comparator (which reboxes per the cell's element kind), then write
//! the sorted words from index 0 and the hole words after them (every
//! word keeps its one owner, so no refcount moves) and mark the tail.
//! A comparator that throws leaves the array as it was — the spec
//! throws before its first Set — with the marks put back. A typed cell
//! holds no accessor index (`define_index_accessor` refuses one), so
//! nothing here can run user code but the comparator.

use core::ffi::c_void;

use crate::layout::{ARR_LEN_OFF, arr_data};

/// Offset of the deque `head_offset` u32 (T-13.5).
const ARR_HDR_HEAD_OFF: usize = 20;

/// Sort a typed `arr` whose holes are marked. The caller has run the
/// frozen guard and marked the cell's element kind; `(cb_env,
/// cb_entry, has_cb)` is [`crate::sort::__torajs_arr_any_sort`]'s
/// callback contract.
///
/// # Safety
/// `arr` is a live typed `Tag::Arr` cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_sort_typed_holes(
    arr: *mut u8,
    cb_env: *mut c_void,
    cb_entry: u64,
    has_cb: i64,
) {
    unsafe {
        let len = *(arr.add(ARR_LEN_OFF) as *const u64);
        let holes = crate::define_hole::take_holes(arr.cast(), 0, len);
        let head = *(arr.add(ARR_HDR_HEAD_OFF) as *const u32) as usize;
        let slots = arr_data(arr).add(head * 8) as *mut u64;
        let mut words: Vec<u64> = Vec::with_capacity(len as usize);
        let mut tail: Vec<u64> = Vec::with_capacity(holes.len());
        let mut next_hole = holes.iter().peekable();
        for i in 0..len {
            let w = *slots.add(i as usize);
            if next_hole.next_if_eq(&&i).is_some() {
                tail.push(w);
            } else {
                words.push(w);
            }
        }
        let present = words.len();
        crate::sort::any_sort_slots(arr, words.as_mut_ptr(), present, cb_env, cb_entry, has_cb);
        if __torajs_throw_check() != 0 {
            for h in holes {
                crate::define_hole::mark_hole_range(arr.cast(), h, h + 1);
            }
            return;
        }
        words.extend(tail);
        core::ptr::copy_nonoverlapping(words.as_ptr(), slots, len as usize);
        crate::define_hole::mark_hole_range(arr.cast(), present as u64, len);
    }
}

unsafe extern "C" {
    /// torajs-throw — pending-throw probe (the comparator's).
    fn __torajs_throw_check() -> i64;
}
