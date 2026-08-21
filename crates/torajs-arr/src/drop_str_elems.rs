//! One call for a whole `Array<Str>` / `Array<Substr>` release —
//! RFC 20260821 attack A2.
//!
//! `ssa_lower` used to emit the element walk into the user's own code:
//! a loop with an alloca'd induction variable, and one opaque
//! `__torajs_str_drop` per slot. Measured on `split-only-100k`
//! (seven inline substrings of a string literal, released 100k times),
//! that loop costs 1.1 ms wall — and an A/B that empties the loop body
//! shows **the bookkeeping is the larger half**: 0.7 ms for the loop
//! itself against 0.4 ms for the seven calls.
//!
//! That split is why the fix is one call rather than a cheaper call.
//! An earlier attempt emitted the flag tests inline at each slot to
//! skip the call; it bought 0.1 ms, because reading three header
//! fields and branching three times costs about what the call did,
//! and the loop was still there. The loop has to go.
//!
//! Here the walk is ordinary compiled code — registers, no alloca, no
//! per-slot reload of the induction variable — and the flag tests come
//! for free alongside it. In the shape that motivated this, every slot
//! answers "nothing is owed" from two loads, so a hundred thousand
//! iterations make zero calls out of this crate.

use core::ffi::c_void;

use torajs_rc::{FLAG_STATIC_LITERAL, HeapHeader};
use torajs_str::substr::{FLAG_SUBSTR_INLINE, FLAG_SUBSTR_VIEW, SUBSTR_PARENT_OFF};

use crate::drop::__torajs_arr_drop;
use crate::layout::{ARR_CAP_OFF, arr_data, arr_live_extent};

unsafe extern "C" {
    /// The definition of correct for a single element; this module
    /// only skips calling it in the cases it can prove are no-ops.
    /// Signature matches the sibling declaration in `define.rs`.
    fn __torajs_str_drop(s: *mut c_void);
}

/// True when releasing this cell would do nothing at all, decided from
/// the cell's own header plus (for a substring view) its parent's.
///
/// Mirrors `__torajs_str_drop`'s own opening tests, which is what makes
/// skipping sound:
///
///   - a `FLAG_STATIC_LITERAL` cell returns immediately there;
///   - an inline substring never frees itself — its storage belongs to
///     the enclosing split block — so its whole body is `drop_parent`,
///     and `drop_parent` returns at once on a null or literal parent
///     (`7eb12179`).
///
/// A `FLAG_SUBSTR_VIEW` cell that is not inline owns itself and still
/// has to go through the kernel, as does any live-parent substring and
/// any ordinary owned Str.
#[inline]
unsafe fn release_is_noop(cell: *mut u8) -> bool {
    let flags = unsafe { (*(cell as *const HeapHeader)).flags };
    if flags & FLAG_STATIC_LITERAL != 0 {
        return true;
    }
    if flags & FLAG_SUBSTR_INLINE == 0 {
        // Owned Str, or a self-owning view: the kernel has real work.
        let _ = FLAG_SUBSTR_VIEW;
        return false;
    }
    let parent = unsafe { *(cell.add(SUBSTR_PARENT_OFF) as *const *mut u8) };
    if parent.is_null() {
        return true;
    }
    unsafe { (*(parent as *const HeapHeader)).flags & FLAG_STATIC_LITERAL != 0 }
}

/// Release an array whose elements are `Str` or `Substr`, then the
/// array itself. Replaces the SSA-emitted element walk plus its
/// trailing `__torajs_arr_drop`.
///
/// The `refcount == 1` gate is the same one `ssa_lower` emitted: the
/// element references belong to the array block, so only the last
/// owner walks them (RFC 20260704 S6). Slots are walked over
/// `arr_live_extent`, not `len` — a sparse tail has no storage past
/// the extent.
///
/// # Safety
///
/// `p` is null or a live array cell whose slots hold `Str`-tagged
/// pointers (owned strings or substring views).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_drop_str_elems(p: *mut c_void) {
    if p.is_null() {
        return;
    }
    let cell = p as *mut u8;
    // SAFETY: caller contract — live array cell, universal header @0.
    let header = unsafe { &*(cell as *const HeapHeader) };
    if header.refcount == 1 {
        let extent = unsafe { arr_live_extent(cell) };
        if extent != 0 {
            // cap:u32 | head:u32 packed at ARR_CAP_OFF (T-13.5 deque).
            let head = unsafe { *(cell.add(ARR_CAP_OFF + 4) as *const u32) } as usize;
            let slots = unsafe { arr_data(cell).add(head * 8) as *const *mut u8 };
            for i in 0..extent as usize {
                let elem = unsafe { *slots.add(i) };
                if elem.is_null() {
                    continue;
                }
                if unsafe { release_is_noop(elem) } {
                    continue;
                }
                unsafe { __torajs_str_drop(elem as *mut c_void) };
            }
        }
    }
    unsafe { __torajs_arr_drop(p) };
}
