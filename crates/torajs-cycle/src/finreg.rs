//! `Tag::FinalizationRegistry` mirror for the trial-deletion walk.
//!
//! A registry owns its cleanup callback and every cell's held value.
//! A callback that closes over its own registry — the usual way a
//! cleanup re-registers — is a two-cell ring, and so is a held value
//! that names the registry.
//!
//! Mirror of `torajs-weak/src/finreg.rs`:
//!
//! ```text
//! FinalizationRegistry cell (72B)
//! offset | field
//! -------|------
//!   0    | universal heap header (8B)
//!   8    | cleanup (AnyValue)         — slot 0
//!  16    | cells (*mut *mut FrCell)   — slots 1 ..= cells_len
//!  24    | cells_len (u32)
//!  32    | props (bag)
//!  40    | pending (*mut *mut FrCell) — the slots after the live ones
//!  48    | pending_head (u32)
//!  52    | pending_len (u32)
//!
//! FrCell: held (AnyValue) at +16
//! ```
//!
//! Target and token are observed, never owned, and are not edges.

use core::ffi::c_void;

use crate::layout::nan_box_is_cell_like;

/// `torajs_rc::Tag::FinalizationRegistry` — the stable wire value.
pub const TAG_FINREG: u16 = 31;

const CLEANUP_OFF: usize = 8;
const CELLS_OFF: usize = 16;
const CELLS_LEN_OFF: usize = 24;
const PENDING_OFF: usize = 40;
const PENDING_HEAD_OFF: usize = 48;
const PENDING_LEN_OFF: usize = 52;
const CELL_HELD_OFF: usize = 16;

#[inline]
unsafe fn u32_at(p: *mut c_void, off: usize) -> u32 {
    unsafe { *((p as *const u8).add(off) as *const u32) }
}

/// Walkable slot count: the callback plus one per cell.
///
/// # Safety
/// `p` must be a live registry.
#[inline]
pub unsafe fn finreg_child_count(p: *mut c_void) -> u64 {
    unsafe {
        1 + u32_at(p, CELLS_LEN_OFF) as u64
            + (u32_at(p, PENDING_LEN_OFF) - u32_at(p, PENDING_HEAD_OFF)) as u64
    }
}

/// The word slot `i` addresses.
#[inline]
unsafe fn slot_ptr(p: *mut c_void, i: u64) -> *mut u64 {
    unsafe {
        if i == 0 {
            return (p as *mut u8).add(CLEANUP_OFF) as *mut u64;
        }
        let live = u32_at(p, CELLS_LEN_OFF) as u64;
        let (vec_off, k) = if i <= live {
            (CELLS_OFF, i - 1)
        } else {
            (
                PENDING_OFF,
                i - 1 - live + u32_at(p, PENDING_HEAD_OFF) as u64,
            )
        };
        let vec = *((p as *const u8).add(vec_off) as *const *const *mut u8);
        (*vec.add(k as usize)).add(CELL_HELD_OFF) as *mut u64
    }
}

/// The cell slot `i` holds, or NULL for an immediate.
///
/// # Safety
/// `p` is a live registry and `i` below [`finreg_child_count`].
#[inline]
pub unsafe fn finreg_child_at(p: *mut c_void, i: u64) -> *mut c_void {
    let v = unsafe { *slot_ptr(p, i) } as *mut c_void;
    if nan_box_is_cell_like(v) {
        v
    } else {
        core::ptr::null_mut()
    }
}

/// Zero slot `i` — the first-sweep cycle break; the registry's drop
/// skips a zero word like any other non-cell.
///
/// # Safety
/// Same as [`finreg_child_at`].
#[inline]
pub unsafe fn finreg_slot_clear(p: *mut c_void, i: u64) {
    unsafe { *slot_ptr(p, i) = 0 };
}
