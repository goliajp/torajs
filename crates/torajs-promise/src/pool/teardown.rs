//! The rc-aware drop kernel: a promise losing its last reference
//! tears down what it still owns and goes back to the pool.

use core::ffi::c_void;
use core::ptr;

use super::{FLAG_STATIC_LITERAL, FLAG_SUBCLASSED, free, promise_release_};
use crate::layout::{
    PromiseCb, RX_CLOSURE, RX_NONE, STATE_PENDING, as_promise, inline_link_derived,
};
use crate::reaction::candidate::{cycle_candidate, offer_root};
use torajs_rc::{FLAG_BUFFERED, rc_dec_cell};

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
/// return Promise to pool (or free if pool full).
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
        // Still referenced. Only a cell that can close a ring is a
        // candidate root (`reaction::candidate`).
        if unsafe { cycle_candidate(pp) } {
            unsafe { offer_root(pp) };
        }
        return;
    }
    unsafe {
        // Defensive: STATIC_LITERAL Promise (none in practice today;
        // the universal drop dispatch sees the bit and skips, but
        // re-check here to keep the invariant if dispatch path
        // changes).
        if (*pp).header.flags & FLAG_STATIC_LITERAL != 0 {
            return;
        }
        // Unfired reactions in this cell's list: a heap record is
        // freed, its block released when `ops` describes it (an opaque
        // block stays with the kernel that minted it); an inline
        // derived cell has its back-pointer to this dying cell cut
        // and the list's stake on it released.
        let mut link = (*pp).callbacks;
        while !link.is_null() {
            let derived = inline_link_derived(link);
            if derived.is_null() {
                let next = (*link).next;
                if !(*link).ops.is_null() {
                    ((*(*link).ops).release)((*link).arg);
                }
                free(link as *mut c_void, core::mem::size_of::<PromiseCb>());
                link = next;
            } else {
                let next = (*derived).rx_next;
                (*derived).rx_next = ptr::null_mut();
                (*derived).rx_source = ptr::null_mut();
                __torajs_promise_drop(derived as *mut c_void);
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
            (*pp).rx_next = ptr::null_mut();
            if kind & RX_CLOSURE != 0 && !cb.is_null() {
                __torajs_value_drop_heap(cb);
            }
        }
        // Heap-typed resolved value gets a type-specific drop.
        if (*pp).value_is_heap != 0 && (*pp).state != STATE_PENDING && (*pp).value != 0 {
            __torajs_value_drop_heap((*pp).value as *mut c_void);
        }
        if (*pp).header.flags & FLAG_SUBCLASSED != 0 {
            __torajs_subclass_drop_entry(p);
        }
        // Expando props dynobj (defineProperty landings) — the
        // universal dispatcher routes it to the dynobj drop; NULLed
        // so a pool re-issue starts clean.
        if !(*pp).props.is_null() {
            __torajs_value_drop_heap((*pp).props);
            (*pp).props = ptr::null_mut();
        }
        // Scrub from the root buffer before the memory goes away: a
        // cell buffered above that later normal-drops to zero would
        // leave a dangling candidate. The bit is read here so the
        // common never-buffered promise skips the call.
        if (*pp).header.flags & FLAG_BUFFERED != 0 {
            __torajs_cycle_unbuffer(p);
        }
        promise_release_(pp);
    }
}
