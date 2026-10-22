//! The rc-aware drop kernel: a promise losing its last reference
//! tears down what it still owns and goes back to the allocator. The
//! derived cells its death leaves unreferenced are torn down in a
//! loop ([`free_chain`]), so the depth of a pending chain is never
//! the depth of the native stack.

use core::ffi::c_void;
use core::ptr;

use super::{FLAG_STATIC_LITERAL, FLAG_SUBCLASSED, free, promise_release_};
use crate::layout::{
    CB_FRAME_TAG, CB_INLINE_TAG, Promise, PromiseCb, RX_CLOSURE, RX_NONE, STATE_PENDING,
    as_promise, frame_cell, inline_cell,
};
use crate::reaction::candidate::{cycle_candidate, offer_root};
use torajs_rc::{DropPolicy, FLAG_BUFFERED, HeapHeader, rc_dec_cell};

unsafe extern "C" {
    /// Universal-drop dispatcher in runtime_str.c. Routes the heap
    /// pointer to its type-specific drop helper.
    fn __torajs_value_drop_heap(p: *mut c_void);
    /// torajs-cycle — cycle-root buffer scrub (the push is
    /// `reaction::candidate::offer_root`'s).
    fn __torajs_cycle_unbuffer(p: *mut c_void);
    /// torajs-meta — scrub a dying exotic-subclass instance's
    /// identity entry (RFC 20260730 blade 0); gated on
    /// `FLAG_SUBCLASSED` so plain promises never call out.
    fn __torajs_subclass_drop_entry(p: *mut c_void);
}

/// rc-aware drop. Called from `value_drop_heap`'s TAG_PROMISE case.
/// On last owner: free residual cb list, drop heap value if any,
/// give the block back to the allocator.
///
/// # Safety
/// `p` is NULL or a live Promise cell; the caller gives up its reference.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_drop(p: *mut c_void) {
    if p.is_null() {
        return;
    }
    let pp = as_promise(p);
    if !unsafe { rc_dec_cell(p) } {
        unsafe { released_alive(pp) };
        return;
    }
    unsafe { free_dead(pp) };
}

/// [`__torajs_promise_drop`] for a caller that has already seen that
/// `p` is a live cell, not a static literal, whose count is at most
/// one: the typed release torajs-codegen opens at a call site sends
/// the last reference here, past the tests it already made. The
/// header's release still reports an underflow and runs the WeakRef
/// hook.
///
/// # Safety
/// `p` is a live non-static Promise cell with a count of at most one;
/// the caller gives up its reference.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_drop_last(p: *mut c_void) {
    let pp = as_promise(p);
    if unsafe { (*(p as *mut HeapHeader)).dec_ref_counted() } == DropPolicy::Free {
        unsafe { free_dead(pp) };
    } else {
        unsafe { released_alive(pp) };
    }
}

/// [`__torajs_promise_drop`] after the last reference went. Out of
/// line, so a release that leaves the cell alive — most of them — runs
/// without the frame the teardown needs.
#[inline(never)]
unsafe fn free_dead(pp: *mut Promise) {
    // A cell that owns nothing — no reaction list, no inline
    // reaction, no heap value, no expando, no identity entry, never
    // buffered — has nothing for `free_cell` to release, so it goes
    // straight back to the allocator, without the teardown's frame. The
    // STATIC_LITERAL bit sends a cell to the general path, which
    // keeps it.
    let owns_nothing = unsafe {
        (*pp).header.flags & (FLAG_STATIC_LITERAL | FLAG_SUBCLASSED | FLAG_BUFFERED) == 0
            && (*pp).callbacks.is_null()
            && (*pp).rx_kind == RX_NONE
            && (*pp).props.is_null()
            && ((*pp).value_is_heap == 0 || (*pp).state == STATE_PENDING || (*pp).value == 0)
    };
    if owns_nothing {
        unsafe { promise_release_(pp) };
        return;
    }
    unsafe { free_owning(pp) };
}

/// [`free_dead`] for a cell that may own something. The cell heads
/// the same loop the derived cells its death frees go through, so the
/// teardown is laid out once.
#[inline(always)]
unsafe fn free_owning(pp: *mut Promise) {
    // Defensive: STATIC_LITERAL Promise (none in practice today; the
    // universal drop dispatch sees the bit and skips, but re-check
    // here to keep the invariant if dispatch path changes).
    if unsafe { (*pp).header.flags } & FLAG_STATIC_LITERAL != 0 {
        return;
    }
    unsafe { free_chain(pp) };
}

/// A release left `pp` referenced. Only a cell that can close a ring
/// is a candidate root (`reaction::candidate`).
#[inline(always)]
unsafe fn released_alive(pp: *mut Promise) {
    if unsafe { cycle_candidate(pp) } {
        unsafe { offer_root(pp) };
    }
}

/// Free `pp`, which just lost its last reference. A derived cell its
/// list holds (inline, or through a record's block) that the release
/// leaves unreferenced goes to `on_dead` instead of being freed here:
/// [`free_chain`] collects such cells on a stack rather than in nested
/// calls.
#[inline(always)]
unsafe fn free_cell(pp: *mut Promise, mut on_dead: impl FnMut(*mut Promise)) {
    unsafe {
        // Unfired reactions in this cell's list: a heap record is
        // freed, its block released when `ops` describes it (an
        // opaque block stays with the kernel that minted it) and the
        // block's stake on its derived promise let go here; an inline
        // derived cell has its back-pointer to this dying cell cut
        // and the list's stake on it let go; a parked async frame the
        // same, its result left pending for good (`frame.rs`).
        let mut link = (*pp).callbacks;
        while !link.is_null() {
            if link as usize & CB_INLINE_TAG != 0 {
                let derived = inline_cell(link);
                link = (*derived).rx_next;
                (*derived).rx_source = ptr::null_mut();
                if rc_dec_cell(derived as *mut c_void) {
                    on_dead(derived);
                } else {
                    (*derived).rx_next = ptr::null_mut();
                    released_alive(derived);
                }
            } else if link as usize & CB_FRAME_TAG != 0 {
                let frame = frame_cell(link);
                link = (*frame).next;
                crate::frame::source_died(frame);
            } else {
                let r = link;
                let next = (*r).next;
                let ops = (*r).ops;
                if !ops.is_null() {
                    let held = ((*ops).release)((*r).arg) as *mut Promise;
                    if !held.is_null() {
                        if rc_dec_cell(held as *mut c_void) {
                            on_dead(held);
                        } else {
                            released_alive(held);
                        }
                    }
                }
                free(r as *mut c_void, core::mem::size_of::<PromiseCb>());
                link = next;
            }
        }
        (*pp).callbacks = ptr::null_mut();
        // An unfired inline reaction this cell carries: release an
        // owned closure env. Its `rx_source` is a back-pointer, not
        // a stake — a queued reaction, the only kind that holds its
        // source, cannot reach here (`reaction.rs`, "Ownership").
        if (*pp).rx_kind != RX_NONE {
            let cb = (*pp).rx_cb;
            let kind = (*pp).rx_kind;
            (*pp).rx_kind = RX_NONE;
            (*pp).rx_source = ptr::null_mut();
            (*pp).rx_cb = ptr::null_mut();
            if kind & RX_CLOSURE != 0 && !cb.is_null() {
                __torajs_value_drop_heap(cb);
            }
        }
        // Heap-typed resolved value gets a type-specific drop.
        if (*pp).value_is_heap != 0 && (*pp).state != STATE_PENDING && (*pp).value != 0 {
            __torajs_value_drop_heap((*pp).value as *mut c_void);
        }
        if (*pp).header.flags & FLAG_SUBCLASSED != 0 {
            __torajs_subclass_drop_entry(pp as *mut c_void);
        }
        // Expando props dynobj (defineProperty landings) — the
        // universal dispatcher routes it to the dynobj drop; NULLed
        // so a reissued block starts clean.
        if !(*pp).props.is_null() {
            __torajs_value_drop_heap((*pp).props);
            (*pp).props = ptr::null_mut();
        }
        // Scrub from the root buffer before the memory goes away: a
        // cell buffered above that later normal-drops to zero would
        // leave a dangling candidate. The bit is read here so the
        // common never-buffered promise skips the call.
        if (*pp).header.flags & FLAG_BUFFERED != 0 {
            __torajs_cycle_unbuffer(pp as *mut c_void);
        }
        promise_release_(pp);
    }
}

/// Free `first`, which just lost its last reference, and every derived
/// cell its death leaves unreferenced, and theirs in turn. They wait on
/// a stack threaded through their `rx_next` words (unused once a cell
/// is out of its source's list, and never used by a cell a record's
/// block holds; a cell at rc 0 is in no list, as a list entry holds a
/// reference), so a pending chain of a million links released at its
/// head is a million iterations here, not a million nested calls.
#[inline(never)]
unsafe fn free_chain(first: *mut Promise) {
    let mut pp = first;
    let mut dead: *mut Promise = ptr::null_mut();
    loop {
        unsafe {
            free_cell(pp, |d| {
                (*d).rx_next = dead as *mut PromiseCb;
                dead = d;
            });
            if dead.is_null() {
                return;
            }
            pp = dead;
            dead = (*pp).rx_next as *mut Promise;
            (*pp).rx_next = ptr::null_mut();
        }
    }
}
