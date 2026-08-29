//! What the iterator family owns, for the trial-deletion walk.
//!
//! `MapIter` and `ArrIter` each hold one strong reference to what
//! they iterate — `MapIter::map`, `ArrIter::arr` — taken so that
//! iteration stays valid past the caller's binding drop. Rotation 528
//! gave both tags their lazy property bag, and that bag has been the
//! only child the collector ever enumerated for them; the reference
//! the cell exists to hold was not an edge the walk could follow.
//!
//! An iterator that can be reached from its own source is an ordinary
//! ring — `const it = a.values(); a.push(it)` — two cells, no
//! closure, nothing exotic.
//!
//! A third shape rides along at the bottom of this file: the
//! §27.1.4.x Iterator Helper, whose four AnyValue slots hold the
//! underlying iterator, the captured callback, flatMap's current
//! inner iterator and the cached `next` method. Same question, wider
//! cell.
//!
//! Both iterator layouts put the source at the same offset, so one
//! arm serves both tags:
//!
//! ```text
//! MapIter (40B)                  ArrIter (40B)
//! offset | field                 offset | field
//! -------|------                 -------|------
//!   0    | heap header             0    | heap header
//!   8    | map    — slot 0         8    | arr    — slot 0
//!  16    | cursor                 16    | cursor
//!  24    | kind + pad             24    | kind + family
//!  32    | props (the bag)        32    | props (the bag)
//! ```
//!
//! Mirrors `torajs-collections/src/iter.rs` and
//! `torajs-arr/src/iter.rs`. Unlike [`crate::proxy`]'s two slots
//! these are raw cell pointers, not NaN-boxed AnyValues, so the
//! read needs no immediate filter — a null pointer is the only
//! "nothing here" this slot can hold.
//!
//! Both tags are [`crate::layout_bag::corpse_takes_own_destructor`]
//! shapes, so a cleared slot is exactly what the corpse's own
//! destructor needs to see: `iter_drop` skips a null source rather
//! than releasing an edge this collect already accounted for.

use core::ffi::c_void;

use crate::layout::nan_box_is_cell_like;
use crate::layout_bag::{TAG_ARR_ITER, TAG_ITER_HELPER, TAG_MAP_ITER};

/// Byte offset of `MapIter::map` / `ArrIter::arr`, directly after
/// the universal header in both layouts.
const ITER_SRC_OFF: usize = 8;

/// The child index [`crate::children::for_each_child`] yields the
/// source under. Neither iterator cell has an entry table, so index
/// 0 is free — the props bag keeps its own `PROPS_SLOT_INDEX`
/// sentinel.
pub const ITER_SRC_SLOT: u64 = 0;

/// True for the two stateful iterator cells that own their source.
#[inline]
pub fn is_iter_cell(type_tag: u16) -> bool {
    matches!(type_tag, TAG_MAP_ITER | TAG_ARR_ITER)
}

/// The Map or Array this iterator walks, or NULL once the source has
/// been released (a finished iterator, or a slot this collect has
/// already broken).
///
/// # Safety
/// `p` must be a live MapIter or ArrIter cell — check
/// [`is_iter_cell`] on its tag first.
#[inline]
pub unsafe fn iter_src(p: *mut c_void) -> *mut c_void {
    unsafe { *((p as *const u8).add(ITER_SRC_OFF) as *const *mut c_void) }
}

/// Null the source slot — `collect_white`'s first-sweep cycle break.
///
/// # Safety
/// Same as [`iter_src`].
#[inline]
pub unsafe fn iter_src_clear(p: *mut c_void) {
    unsafe { *((p as *mut u8).add(ITER_SRC_OFF) as *mut *mut c_void) = core::ptr::null_mut() };
}

/// The four AnyValue slots an Iterator Helper owns, in walk order:
/// `underlying` (+8), `fn` (+16), `inner` (+40, flatMap's current
/// inner iterator) and `next` (+48, the GetIteratorDirect cache).
/// `counter` (+24) is a number, and the kind / alive / running bytes
/// at +32 are not values at all.
///
/// Mirror of `torajs-anyvalue/src/iter_helper.rs`; the order matches
/// that file's own teardown, which releases exactly these four.
const HELPER_SLOT_OFFS: [usize; 4] = [8, 16, 40, 48];

/// Walkable slot count for a helper cell.
pub const HELPER_CHILD_COUNT: u64 = 4;

/// True for `Tag::IterHelper` — §27.1.4.x's lazy helpers
/// (`map` / `filter` / `take` / `drop` / `flatMap` / `concat` /
/// `zip` / …), all one layout.
#[inline]
pub fn is_iter_helper(type_tag: u16) -> bool {
    type_tag == TAG_ITER_HELPER
}

/// Helper slot `i`, or NULL when it holds an immediate. Unlike the
/// two iterator cells above these are NaN-boxed AnyValues, and some
/// kinds deliberately park a number there — `take` and `drop` keep
/// their remaining count in the `fn` slot — so the immediate filter
/// is doing real work here, not defending against a malformed heap.
///
/// # Safety
/// `p` must be a live IterHelper cell and `i` below
/// [`HELPER_CHILD_COUNT`].
#[inline]
pub unsafe fn helper_child_at(p: *mut c_void, i: u64) -> *mut c_void {
    let v = unsafe { *(helper_slot_ptr(p, i) as *const u64) } as *mut c_void;
    if nan_box_is_cell_like(v) {
        v
    } else {
        core::ptr::null_mut()
    }
}

/// Zero helper slot `i` — the same cycle break [`iter_src_clear`]
/// performs, and safe for the same reason: a zero word is not a cell
/// to `__torajs_anyv_rc_dec`, which is exactly what the helper's own
/// teardown calls on all four of these slots.
///
/// # Safety
/// Same as [`helper_child_at`].
#[inline]
pub unsafe fn helper_slot_clear(p: *mut c_void, i: u64) {
    unsafe { *helper_slot_ptr(p, i) = 0 };
}

#[inline]
unsafe fn helper_slot_ptr(p: *mut c_void, i: u64) -> *mut u64 {
    unsafe { (p as *mut u8).add(HELPER_SLOT_OFFS[i as usize]) as *mut u64 }
}
