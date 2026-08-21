//! Single-pass build for the hot split shape — a Latin-1 string cut
//! on one byte — used by [`super::ops::__torajs_str_split`] before
//! its general two-pass build (rotation 469).

use crate::split::ops::{split_init_inline, write_arr_header};
use crate::split::pool::{self, ARR_HDR_SIZE};
use crate::substr::SUBSTR_SIZE;

/// Single-pass build for a Latin-1 string cut on one byte: one scan
/// records the separator positions, the block is sized from their
/// count, and the cells are filled from the recorded positions — no
/// second walk over the text (rotation 469).
///
/// The two-pass build this replaces on the hot shape cost, measured
/// on `"3 4 + 2 * 5 +".split(" ")` in the `split_micro` harness, 2.4
/// ns for its count pass (the `filter().count()` reduce was
/// auto-vectorised into a chain of u64 widenings that loses to thirteen
/// plain compares) and 7.3 ns for its fill pass, whose loop re-read the
/// cell base from the stack on every match.
///
/// Answers `None` when the separator count overflows the position
/// buffer or the text is longer than a byte position can address;
/// the caller then takes the general build, which has no such limits.
///
/// # Safety
///
/// `s` is the parent Str heap pointer and `payload` its Latin-1 bytes.
#[inline(always)]
pub(super) unsafe fn split_byte_sep<const PARENT_RC: bool>(
    s: *const u8,
    payload: &[u8],
    target: u8,
) -> Option<*mut u8> {
    const MAX_SEPS: usize = 64;
    let len = payload.len();
    if len > u8::MAX as usize {
        return None;
    }
    let mut pos = [0u8; MAX_SEPS];
    let mut n: usize = 0;
    for (i, &b) in payload.iter().enumerate() {
        if b == target {
            if n == MAX_SEPS {
                return None;
            }
            pos[n] = i as u8;
            n += 1;
        }
    }
    let oc = (n + 1) as u64;
    let block = pool::alloc(oc);
    unsafe { write_arr_header(block, oc) };
    let mut cell = unsafe { block.as_ptr().add(ARR_HDR_SIZE + (n + 1) * 8) };
    let mut slot = unsafe { block.as_ptr().add(ARR_HDR_SIZE) as *mut *mut u8 };
    let mut start: usize = 0;
    for &p in &pos[..n] {
        let p = p as usize;
        unsafe {
            split_init_inline::<PARENT_RC>(cell, slot, s, start as u64, (p - start) as u64);
            cell = cell.add(SUBSTR_SIZE);
            slot = slot.add(1);
        }
        start = p + 1;
    }
    unsafe {
        split_init_inline::<PARENT_RC>(cell, slot, s, start as u64, (len - start) as u64);
    }
    Some(block.as_ptr())
}
