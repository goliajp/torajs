//! `Tag::AccessorPair` mirror for the trial-deletion walk.
//!
//! An AccessorPair owns its getter and setter closures. A class whose
//! prototype carries an accessor whose face holds the class back is a
//! ring of four — class → prototype → pair → face → class — and every
//! evaluation of a unit class with a private accessor makes one. It
//! was invisible: the collector had no arm for the tag, so the pair
//! was a leaf the second sweep released, never a place the walk could
//! pass through, and a face that held its class could not be minted
//! mortal without leaking the whole ring.
//!
//! Mirror of `torajs-dynobj/src/accessor.rs`:
//!
//! ```text
//! AccessorPair cell (32B, sized `__torajs_calloc`)
//! offset | field
//! -------|------
//!   0    | universal heap header (8B)
//!   8    | get_closure (raw closure ptr, 0 = absent) — slot 0
//!  16    | set_closure (raw closure ptr, 0 = absent) — slot 1
//!  24    | kinds (packed bytes, not a cell)
//! ```
//!
//! The block is a sized allocation, so a corpse must go back through
//! `__torajs_accessor_drop` ([`crate::layout_bag::corpse_takes_own_destructor`])
//! rather than the bare `free` — the same reason a Proxy does.

use core::ffi::c_void;

use crate::layout::{FLAG_STATIC_LITERAL, HeapHeader, nan_box_is_cell_like};

/// `torajs_rc::Tag::AccessorPair` mirror — the stable wire value.
pub const TAG_ACCESSOR_PAIR: u16 = 18;

/// `get_closure`; `set_closure` sits 8 bytes on.
const ACC_GET_OFF: usize = 8;

/// Walkable slot count — getter and setter, always both.
pub const ACCESSOR_CHILD_COUNT: u64 = 2;

/// True when `p` is an AccessorPair with at least one closure. A
/// pair whose descriptor omitted both (`{ get: undefined }`) has
/// nothing to walk.
///
/// # Safety
/// `p` is NULL or a live heap block with the universal header.
#[inline]
pub unsafe fn is_visitable_accessor(p: *mut c_void) -> bool {
    if p.is_null() {
        return false;
    }
    let header = unsafe { &*(p as *const HeapHeader) };
    if header.flags & FLAG_STATIC_LITERAL != 0 || header.type_tag != TAG_ACCESSOR_PAIR {
        return false;
    }
    (0..ACCESSOR_CHILD_COUNT).any(|i| !unsafe { accessor_child_at(p, i) }.is_null())
}

/// The closure slot `i` holds (0 = getter, 1 = setter), or NULL when
/// that accessor is absent.
///
/// # Safety
/// `p` must be a live AccessorPair and `i` below
/// [`ACCESSOR_CHILD_COUNT`].
#[inline]
pub unsafe fn accessor_child_at(p: *mut c_void, i: u64) -> *mut c_void {
    let v = unsafe { *(slot_ptr(p, i) as *const u64) } as *mut c_void;
    if nan_box_is_cell_like(v) {
        v
    } else {
        core::ptr::null_mut()
    }
}

/// Zero slot `i` — `collect_white`'s first-sweep cycle break. A zero
/// word is an absent accessor to `__torajs_accessor_drop`, so the
/// corpse's own destructor in [`crate::defer`] pass B skips the slot
/// rather than releasing an edge this collect already accounted for.
///
/// # Safety
/// Same as [`accessor_child_at`].
#[inline]
pub unsafe fn accessor_slot_clear(p: *mut c_void, i: u64) {
    unsafe { *slot_ptr(p, i) = 0 };
}

#[inline]
unsafe fn slot_ptr(p: *mut c_void, i: u64) -> *mut u64 {
    unsafe { (p as *mut u8).add(ACC_GET_OFF + i as usize * 8) as *mut u64 }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A pair block laid out exactly as `__torajs_accessor_pair_new`
    /// writes it: header, getter, setter, kinds.
    fn pair(get: u64, set: u64, flags: u16) -> [u64; 4] {
        let header = 1u64 | ((TAG_ACCESSOR_PAIR as u64) << 32) | ((flags as u64) << 48);
        [header, get, set, 7]
    }

    #[test]
    fn both_slots_are_children_in_order() {
        let mut b = pair(0x1000, 0x2000, 0);
        let p = b.as_mut_ptr() as *mut c_void;
        assert!(unsafe { is_visitable_accessor(p) });
        assert_eq!(unsafe { accessor_child_at(p, 0) } as u64, 0x1000);
        assert_eq!(unsafe { accessor_child_at(p, 1) } as u64, 0x2000);
    }

    #[test]
    fn absent_accessor_is_no_child_and_a_bare_pair_is_not_visitable() {
        let mut b = pair(0, 0x2000, 0);
        let p = b.as_mut_ptr() as *mut c_void;
        assert!(unsafe { accessor_child_at(p, 0) }.is_null());
        assert!(unsafe { is_visitable_accessor(p) });
        let mut none = pair(0, 0, 0);
        assert!(!unsafe { is_visitable_accessor(none.as_mut_ptr() as *mut c_void) });
    }

    #[test]
    fn clearing_a_slot_removes_only_that_edge() {
        let mut b = pair(0x1000, 0x2000, 0);
        let p = b.as_mut_ptr() as *mut c_void;
        unsafe { accessor_slot_clear(p, 0) };
        assert!(unsafe { accessor_child_at(p, 0) }.is_null());
        assert_eq!(unsafe { accessor_child_at(p, 1) } as u64, 0x2000);
        assert_eq!(b[3], 7, "kinds word untouched");
    }

    #[test]
    fn static_literal_pair_is_not_walked() {
        let mut b = pair(0x1000, 0x2000, FLAG_STATIC_LITERAL);
        assert!(!unsafe { is_visitable_accessor(b.as_mut_ptr() as *mut c_void) });
    }
}
