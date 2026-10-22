//! Whether a promise can close a ring — the cycle-root test the drop
//! kernel asks when a release leaves a cell alive.
//!
//! A cell lies on a cycle only if the walk can leave it and come
//! back. A promise's own edges — its bag, a settled heap value, its
//! reaction's closure env — can lead anywhere and are taken as they
//! are. Its callback list leads only into the tree of cells derived
//! from it, and a ring can run through that tree only if some cell in
//! it has an edge out: an env, a bag, a described record's block
//! (`record`), or a parked async frame (`frame`). A cell whose tree
//! has none — every link of a `then` chain with plain function
//! handlers, a million of them in `p = p.then(f)` — is never offered
//! as a root.
//!
//! [`FLAG_RX_ENV`] says the cell's own reaction calls an env, so the
//! test is one header load for the common bare cell. It is set with
//! the reaction and left stale once the reaction ran, which costs the
//! collector one look at a cell with nothing to walk.
//!
//! [`FLAG_LIST_EDGE`] records "the tree has an edge out" on every
//! ancestor of the cell that has it. The ancestors of a derived cell
//! are fixed when it is made (its `rx_source` chain), so the mark is
//! pushed up once, when the edge appears, and stops at the first
//! cell already marked: each cell is marked at most once. It is
//! never cleared; a stale mark costs a look, never a missed ring.
//! The edge appears
//!
//! - with the reaction, for an env (`attach_inline`) or a described
//!   record (`record::attach_record`) — the source is marked;
//! - when an async frame parks in the list (`frame`): its slots can
//!   hold anything, so the source is marked the same way;
//! - for a bag, whenever it is first written: there is no hook at
//!   the many writers, but a bag is written through a reference to
//!   the cell, and that reference is released afterwards, while the
//!   cell is still parked (its source's list holds it). The release
//!   marks the source ([`note_released`]).

use core::ffi::c_void;

use crate::layout::{Promise, RX_NONE, STATE_PENDING};
use crate::pool::FLAG_STATIC_LITERAL;

/// Header flag on a promise: some cell in the tree derived from it
/// has an edge out (module doc). Bit 10 is disjoint-by-tag; see the
/// occupancy map in torajs-rc `flags.rs`.
pub const FLAG_LIST_EDGE: u16 = 1 << 10;

/// Header flag on a promise: its own reaction calls a closure env
/// (module doc). Bit 11, disjoint-by-tag like [`FLAG_LIST_EDGE`].
pub const FLAG_RX_ENV: u16 = 1 << 11;

/// Can this cell close a ring, so that a release leaving it alive must
/// offer it to the cycle collector as a root?
///
/// Every cell torajs-cycle could find on a ring answers yes: the walk
/// it would take (`promise::for_each_promise_child`) is the bag, the
/// settled value, the env and the tree, and the marks cover the last
/// two. A stale mark answers yes for a cell with nothing to walk, and
/// `cycle_buffer`'s own walkability check turns that away.
///
/// torajs-codegen opens this test at a typed release's call site
/// (`promise_drop.rs`); a change here moves there too.
#[inline(always)]
pub(crate) unsafe fn cycle_candidate(pp: *const Promise) -> bool {
    unsafe {
        let flags = (*pp).header.flags;
        flags & FLAG_STATIC_LITERAL == 0
            && (flags & (FLAG_LIST_EDGE | FLAG_RX_ENV) != 0
                || !(*pp).props.is_null()
                || ((*pp).value_is_heap != 0 && (*pp).state != STATE_PENDING && (*pp).value != 0))
    }
}

/// Mark `p` and its ancestors: a cell in the tree under `p` gained an
/// edge out.
#[inline(never)]
pub(crate) unsafe fn mark_list_edge(mut p: *mut Promise) {
    unsafe {
        while !p.is_null() && (*p).header.flags & FLAG_LIST_EDGE == 0 {
            (*p).header.flags |= FLAG_LIST_EDGE;
            p = if (*p).rx_kind != RX_NONE {
                (*p).rx_source as *mut Promise
            } else {
                core::ptr::null_mut()
            };
        }
    }
}

unsafe extern "C" {
    fn __torajs_cycle_buffer(p: *mut c_void);
}

/// A release left the candidate `pp` alive: offer it to the collector.
/// Out of line, so the release path of a bare cell stays as short as
/// the test itself.
#[inline(never)]
pub(crate) unsafe fn offer_root(pp: *mut Promise) {
    unsafe {
        note_released(pp);
        __torajs_cycle_buffer(pp as *mut c_void);
    }
}

/// If the released `pp` is a derived cell with a bag, its source's
/// tree has an edge out.
#[inline(always)]
unsafe fn note_released(pp: *const Promise) {
    unsafe {
        if !(*pp).props.is_null() && (*pp).rx_kind != RX_NONE {
            mark_list_edge((*pp).rx_source as *mut Promise);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{
        HeapHeader, RX_THEN, STATE_FULFILLED, STATE_REJECTED, TAG_PROMISE, inline_link,
    };
    use core::ptr;

    fn cell(flags: u16, state: u8, is_heap: u8, value: i64, props: *mut c_void) -> Promise {
        Promise {
            header: HeapHeader {
                refcount: 1,
                type_tag: TAG_PROMISE,
                flags,
            },
            state,
            value_is_heap: is_heap,
            has_handler: 0,
            value_repr: 0,
            deferred_state: 0,
            rx_kind: RX_NONE,
            rx_ret_repr: 0,
            rx_param_repr: 0,
            value,
            callbacks: ptr::null_mut(),
            props,
            rx_next: ptr::null_mut(),
            rx_source: ptr::null_mut(),
            rx_cb: ptr::null_mut(),
        }
    }

    fn bare() -> Promise {
        cell(0, STATE_PENDING, 0, 0, ptr::null_mut())
    }

    fn boxed(p: Promise) -> *mut Promise {
        Box::into_raw(Box::new(p))
    }

    #[test]
    fn own_edges_make_a_candidate() {
        let bag = 0x1000 as *mut c_void;
        let heap = 0x2000;
        // bagless, primitive-valued or pending: never a root
        assert!(!unsafe { cycle_candidate(&bare()) });
        assert!(!unsafe { cycle_candidate(&cell(0, STATE_FULFILLED, 0, 7, ptr::null_mut())) });
        assert!(!unsafe { cycle_candidate(&cell(0, STATE_PENDING, 1, heap, ptr::null_mut())) });
        assert!(!unsafe { cycle_candidate(&cell(0, STATE_FULFILLED, 1, 0, ptr::null_mut())) });
        // owned settled heap value, either state
        assert!(unsafe { cycle_candidate(&cell(0, STATE_FULFILLED, 1, heap, ptr::null_mut())) });
        assert!(unsafe { cycle_candidate(&cell(0, STATE_REJECTED, 1, heap, ptr::null_mut())) });
        // a live bag alone qualifies
        assert!(unsafe { cycle_candidate(&cell(0, STATE_PENDING, 0, 0, bag)) });
        // a static literal never does
        assert!(!unsafe {
            cycle_candidate(&cell(FLAG_STATIC_LITERAL, STATE_FULFILLED, 1, heap, bag))
        });
        // a reaction's closure env does, a plain handler does not
        let mut d = bare();
        d.rx_kind = RX_THEN;
        d.rx_cb = 0x3000 as *mut c_void;
        assert!(!unsafe { cycle_candidate(&d) });
        d.header.flags |= FLAG_RX_ENV;
        assert!(unsafe { cycle_candidate(&d) });
    }

    #[test]
    fn a_marked_tree_makes_a_candidate() {
        let s = boxed(bare());
        let d = boxed(bare());
        unsafe {
            (*s).callbacks = inline_link(d);
            assert!(!cycle_candidate(s));
            (*s).header.flags |= FLAG_LIST_EDGE;
            assert!(cycle_candidate(s));
        }
    }

    // root <- a <- b: a mark at b's source climbs to the root once
    #[test]
    fn the_mark_climbs_the_source_chain_and_stops_at_a_marked_cell() {
        let root = boxed(bare());
        let a = boxed(bare());
        let b = boxed(bare());
        unsafe {
            (*a).rx_kind = RX_THEN;
            (*a).rx_source = root as *mut c_void;
            (*b).rx_kind = RX_THEN;
            (*b).rx_source = a as *mut c_void;
            mark_list_edge(a);
            assert!((*a).header.flags & FLAG_LIST_EDGE != 0);
            assert!((*root).header.flags & FLAG_LIST_EDGE != 0);
            assert!((*b).header.flags & FLAG_LIST_EDGE == 0);
            // a marked cell ends the climb
            (*root).header.flags &= !FLAG_LIST_EDGE;
            mark_list_edge(a);
            assert!((*root).header.flags & FLAG_LIST_EDGE == 0);
            // a released derived cell with a bag marks its source
            (*b).props = 0x1000 as *mut c_void;
            (*a).header.flags &= !FLAG_LIST_EDGE;
            note_released(b);
            assert!((*a).header.flags & FLAG_LIST_EDGE != 0);
            assert!((*root).header.flags & FLAG_LIST_EDGE != 0);
        }
    }
}
