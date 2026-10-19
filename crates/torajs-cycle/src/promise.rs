//! A Promise's owned edges, for the trial-deletion walk.
//!
//! Mirror of `torajs-promise/src/layout.rs`:
//!
//! ```text
//! Promise cell (64B)
//! offset | field
//! -------|------
//!   0    | universal heap header (8B)
//!   8    | state (u8) — 0 PENDING / 1 FULFILLED / 2 REJECTED
//!   9    | value_is_heap (u8)
//!  10    | has_handler, value_repr, deferred_state, rx_kind,
//!       | rx_ret_repr, rx_param_repr
//!  16    | value (i64) — a cell pointer iff value_is_heap
//!  24    | callbacks (tagged list head: heap PromiseCb records,
//!       | inline derived promises and parked async frames
//!       | interleave; bit 0 marks inline, bit 1 a frame)
//!  32    | props (the bag)
//!  40    | rx_next — this cell's link in its source's list
//!  48    | rx_source — the promise this cell reacts to
//!  56    | rx_cb — handler fn pointer, or an owned closure env
//! ```
//!
//! The edges a promise owns, besides its bag:
//!
//! - its settled value (`Promise.resolve(o); o.p = p` is a ring);
//! - each derived cell parked in its callback list — the list entry
//!   holds one reference on it until the reaction runs — and each
//!   parked async frame, held the same way (`frame`);
//! - the closure env its own unfired reaction calls (`rx_cb` with
//!   `RX_CLOSURE`), which can capture anything, the derived promise
//!   included (`const d = p.then(() => d)`);
//! - what a heap callback record's `arg` block owns, when the record
//!   carries `ops` (torajs-promise `reaction::record`): its `trace`
//!   reports each cell with the address of the word holding it.
//!
//! `rx_source` is not an edge while the reaction waits in its
//! source's list: there it is a back-pointer the source clears when
//! it dies. The source is pinned only from the moment its settlement
//! queues the reaction, and a queued cell is held by the microtask
//! queue, which the walk cannot see, so nothing queued is ever white.
//! The same holds for a described record's pointer to its source. A
//! record without `ops` is not walked: its `arg` word is opaque here,
//! and a ring through one is under-collected, never corrupted.

use core::ffi::c_void;

/// `Promise::state`; `value_is_heap` is the byte after it.
const STATE_OFF: usize = 8;
/// `Promise::value_is_heap`.
const VALUE_IS_HEAP_OFF: usize = 9;
/// `Promise::rx_kind`.
const RX_KIND_OFF: usize = 13;
/// `Promise::value`.
const VALUE_OFF: usize = 16;
/// `Promise::callbacks`.
const CALLBACKS_OFF: usize = 24;
/// `Promise::rx_next`.
const RX_NEXT_OFF: usize = 40;
/// `Promise::rx_source`.
const RX_SOURCE_OFF: usize = 48;
const _: () = assert!(
    crate::frame::FRAME_NEXT_OFF == RX_NEXT_OFF
        && crate::frame::FRAME_AWAITING_OFF == RX_SOURCE_OFF
);
/// `Promise::rx_cb`.
const RX_CB_OFF: usize = 56;
/// `PromiseCb::arg`.
const CB_ARG_OFF: usize = 8;
/// `PromiseCb::next`.
const CB_NEXT_OFF: usize = 16;
/// `PromiseCb::ops`; `ReactionOps::trace` is the first word behind it.
const CB_OPS_OFF: usize = 24;
/// `STATE_PENDING` — a pending promise holds no value yet, and its
/// `value` word is not a pointer.
const STATE_PENDING: u8 = 0;
/// `RX_CLOSURE` — `rx_cb` is an owned closure env.
const RX_CLOSURE: u8 = 4;
/// `CB_INLINE_TAG` — low bit of a list link naming a derived cell.
const CB_INLINE_TAG: usize = 1;
/// `CB_FRAME_TAG` — bit 1 of a list link naming a parked frame.
const CB_FRAME_TAG: usize = 2;
/// Both link tags.
const CB_TAGS: usize = CB_INLINE_TAG | CB_FRAME_TAG;

/// The child index the settled value is yielded under.
pub const PROMISE_VALUE_SLOT: u64 = 0;
/// The child index the reaction's closure env is yielded under. A
/// list entry is yielded under the address of the word that links
/// to it, a record block's cell under its word's address with
/// [`BLOCK_SLOT_TAG`] set — 8-aligned heap addresses, so never 0, 1
/// or the props sentinel, and never each other.
const RX_CB_SLOT: u64 = 1;
/// See [`RX_CB_SLOT`].
const BLOCK_SLOT_TAG: u64 = 2;

/// `ReactionOps::trace`; the visit side is the closure trace ABI.
type BlockTrace = unsafe extern "C" fn(
    i64,
    unsafe extern "C" fn(i64, *mut c_void, *mut c_void, *mut c_void),
    *mut c_void,
);

/// Hands a record block's cells to the walk's `FnMut`.
unsafe extern "C" fn block_visit(_: i64, child: *mut c_void, slot: *mut c_void, ctx: *mut c_void) {
    if child.is_null() || !crate::layout::nan_box_is_cell_like(child) {
        return;
    }
    let f = unsafe { &mut **(ctx as *mut &mut dyn FnMut(u64, *mut c_void)) };
    f(slot as u64 | BLOCK_SLOT_TAG, child);
}

#[inline]
unsafe fn word(p: *mut c_void, off: usize) -> *mut *mut c_void {
    unsafe { (p as *mut u8).add(off) as *mut *mut c_void }
}

/// The settled value word when the drop kernel would release it,
/// else NULL: settled, flagged as heap, non-zero — the kernel's three
/// conditions, verbatim. A cell settled through the any lane is
/// flagged heap whatever it holds, because its value word is a NaN-box
/// the kernel releases through the box-gated helper, so this word may
/// be an immediate (`undefined` is `0x…0a`, a boxed int has its top
/// bits set); [`promise_value`] is the pointer the walk may follow.
#[inline]
unsafe fn promise_value_word(p: *mut c_void) -> *mut c_void {
    let base = p as *const u8;
    if unsafe { *base.add(VALUE_IS_HEAP_OFF) } == 0
        || unsafe { *base.add(STATE_OFF) } == STATE_PENDING
    {
        return core::ptr::null_mut();
    }
    unsafe { *word(p, VALUE_OFF) }
}

/// The settled value when it is a heap cell, else NULL: the walk
/// dereferences what this answers, so an immediate in the value word
/// is not one.
///
/// # Safety
/// `p` must be a live Promise cell.
#[inline]
pub unsafe fn promise_value(p: *mut c_void) -> *mut c_void {
    let v = unsafe { promise_value_word(p) };
    if crate::layout::nan_box_is_cell_like(v) {
        v
    } else {
        core::ptr::null_mut()
    }
}

/// The owned closure env of an unfired reaction, else NULL.
#[inline]
unsafe fn promise_rx_env(p: *mut c_void) -> *mut c_void {
    if unsafe { *(p as *const u8).add(RX_KIND_OFF) } & RX_CLOSURE == 0 {
        return core::ptr::null_mut();
    }
    unsafe { *word(p, RX_CB_OFF) }
}

/// True when a bagless promise may have an edge to walk. A non-empty
/// callback list counts even when it holds only heap records, which
/// yield nothing, and a settled value word counts even when it is an
/// immediate, which yields nothing either: one load or two instead of
/// a list scan or a box test, on a predicate the walk asks of every
/// child it meets. torajs-promise's
/// `cycle_candidate` is the narrower question of whether a cell can
/// close a ring at all, and implies this one.
///
/// # Safety
/// `p` must be a live Promise cell.
#[inline]
pub unsafe fn promise_has_edges(p: *mut c_void) -> bool {
    unsafe {
        !(*word(p, CALLBACKS_OFF)).is_null()
            || !promise_value_word(p).is_null()
            || !promise_rx_env(p).is_null()
    }
}

/// Yield every owned edge but the bag (the caller's props arm walks
/// that). One body for every phase, not one per caller. A callback
/// `f` may clear the slot it is handed — unlink the entry — and the
/// walk carries on with its successor.
///
/// # Safety
/// `p` must be a live Promise cell.
#[inline(never)]
pub(crate) unsafe fn for_each_promise_child(p: *mut c_void, f: &mut dyn FnMut(u64, *mut c_void)) {
    let v = unsafe { promise_value(p) };
    if !v.is_null() {
        f(PROMISE_VALUE_SLOT, v);
    }
    let env = unsafe { promise_rx_env(p) };
    if !env.is_null() {
        f(RX_CB_SLOT, env);
    }
    let mut at = unsafe { word(p, CALLBACKS_OFF) };
    loop {
        let link = unsafe { *at } as usize;
        if link == 0 {
            return;
        }
        if link & CB_TAGS == 0 {
            let cell = link as *mut c_void;
            let ops = unsafe { *word(cell, CB_OPS_OFF) } as *const BlockTrace;
            if !ops.is_null() {
                let mut dyn_f: &mut dyn FnMut(u64, *mut c_void) = &mut *f;
                let ctx = &mut dyn_f as *mut &mut dyn FnMut(u64, *mut c_void) as *mut c_void;
                unsafe { (*ops)(*(word(cell, CB_ARG_OFF) as *const i64), block_visit, ctx) };
            }
            at = unsafe { word(cell, CB_NEXT_OFF) };
            continue;
        }
        // an inline derived cell or a parked frame: a child itself, its
        // link word at the same offset in both
        let cell = (link & !CB_TAGS) as *mut c_void;
        f(at as u64, cell);
        // an unlinked entry's successor now sits in the same word
        if unsafe { *at } as usize == link {
            at = unsafe { word(cell, RX_NEXT_OFF) };
        }
    }
}

/// Clear child slot `i` as [`for_each_promise_child`] yielded it. A
/// record block's word is zeroed, which its `release` reads as
/// accounted for. A list entry is unlinked, and its back-pointer
/// goes with it: the source may be a corpse of this collect, and
/// nothing may name it once the entry has left its list.
///
/// # Safety
/// `p` must be a live Promise cell; `i` must be a slot index
/// [`for_each_promise_child`] yielded for `p`, with no list change
/// in between.
pub(crate) unsafe fn promise_slot_clear(p: *mut c_void, i: u64) {
    match i {
        PROMISE_VALUE_SLOT => unsafe { *(word(p, VALUE_OFF) as *mut u64) = 0 },
        RX_CB_SLOT => unsafe { *word(p, RX_CB_OFF) = core::ptr::null_mut() },
        _ if i & BLOCK_SLOT_TAG != 0 => unsafe { *((i & !BLOCK_SLOT_TAG) as *mut u64) = 0 },
        _ => unsafe {
            let at = i as *mut *mut c_void;
            // a frame's `next` and `awaiting` are at a derived cell's
            // `rx_next` and `rx_source`
            let cell = ((*at) as usize & !CB_TAGS) as *mut c_void;
            *at = *word(cell, RX_NEXT_OFF);
            *word(cell, RX_NEXT_OFF) = core::ptr::null_mut();
            *word(cell, RX_SOURCE_OFF) = core::ptr::null_mut();
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[repr(C, align(8))]
    struct Cell([u64; 8]);

    fn pending() -> Box<Cell> {
        Box::new(Cell([0x0008_0000_0001, 0, 0, 0, 0, 0, 0, 0]))
    }

    fn at(c: &mut Cell) -> *mut c_void {
        c as *mut Cell as *mut c_void
    }

    // source -> d1 (inline) -> record -> d2 (inline)
    #[test]
    fn walk_yields_inline_entries_and_survives_unlinking_them() {
        let mut s = pending();
        let mut d1 = pending();
        let mut d2 = pending();
        let mut rec = [0u64; 4];
        let (sp, d1p, d2p) = (at(&mut s), at(&mut d1), at(&mut d2));
        unsafe {
            *word(sp, CALLBACKS_OFF) = (d1p as usize | CB_INLINE_TAG) as *mut c_void;
            *word(d1p, RX_NEXT_OFF) = rec.as_mut_ptr() as *mut c_void;
            rec[2] = d2p as u64 | CB_INLINE_TAG as u64;
            *word(d1p, RX_SOURCE_OFF) = sp;
            *word(d2p, RX_SOURCE_OFF) = sp;
            assert!(promise_has_edges(sp));
            assert!(!promise_has_edges(d1p));

            let mut seen = Vec::new();
            for_each_promise_child(sp, &mut |_, c| seen.push(c));
            assert_eq!(seen, vec![d1p, d2p]);

            // unlink each entry from inside the walk, as pass A does
            let mut seen = Vec::new();
            for_each_promise_child(sp, &mut |i, c| {
                seen.push(c);
                promise_slot_clear(sp, i);
            });
            assert_eq!(seen, vec![d1p, d2p]);
            // only the record is left, and the entries name nothing
            assert_eq!(*word(sp, CALLBACKS_OFF), rec.as_mut_ptr() as *mut c_void);
            assert_eq!(rec[2], 0);
            assert!((*word(d1p, RX_SOURCE_OFF)).is_null());
            assert!((*word(d2p, RX_SOURCE_OFF)).is_null());
        }
    }

    // a block of three words: two cells and a NULL slot
    unsafe extern "C" fn trace3(
        arg: i64,
        visit: unsafe extern "C" fn(i64, *mut c_void, *mut c_void, *mut c_void),
        ctx: *mut c_void,
    ) {
        let block = arg as *mut *mut c_void;
        for i in 0..3 {
            let slot = unsafe { block.add(i) };
            let child = unsafe { *slot };
            if !child.is_null() {
                unsafe { visit(0, child, slot as *mut c_void, ctx) };
            }
        }
    }

    #[test]
    fn a_described_record_yields_its_block_and_an_opaque_one_does_not() {
        let mut s = pending();
        let mut a = pending();
        let mut b = pending();
        let sp = at(&mut s);
        let (ap, bp) = (at(&mut a), at(&mut b));
        let mut block = [ap, core::ptr::null_mut(), bp];
        let ops: [BlockTrace; 1] = [trace3];
        let mut described = [0u64, block.as_mut_ptr() as u64, 0, ops.as_ptr() as u64];
        let mut opaque = [0u64, block.as_mut_ptr() as u64, 0, 0];
        described[2] = opaque.as_mut_ptr() as u64;
        unsafe {
            *word(sp, CALLBACKS_OFF) = described.as_mut_ptr() as *mut c_void;
            let mut seen = Vec::new();
            for_each_promise_child(sp, &mut |i, c| seen.push((i, c)));
            assert_eq!(seen.len(), 2);
            assert_eq!(seen[0].1, ap);
            assert_eq!(seen[1].1, bp);
            promise_slot_clear(sp, seen[1].0);
            assert!(block[2].is_null());
            assert_eq!(block[0], ap);
        }
        // the opaque record still links the list on
        assert_eq!(opaque[2], 0);
    }

    #[test]
    fn an_any_lane_immediate_value_is_not_an_edge() {
        let mut d = pending();
        let mut v = pending();
        let (dp, vp) = (at(&mut d), at(&mut v));
        unsafe {
            *(dp as *mut u8).add(STATE_OFF) = 1;
            *(dp as *mut u8).add(VALUE_IS_HEAP_OFF) = 1;
            for imm in [0x0000_0000_0000_000a_u64, 0xfffe_0000_0000_0003] {
                *(word(dp, VALUE_OFF) as *mut u64) = imm;
                assert!(promise_value(dp).is_null());
                let mut seen = Vec::new();
                for_each_promise_child(dp, &mut |_, c| seen.push(c));
                assert!(seen.is_empty());
            }
            *word(dp, VALUE_OFF) = vp;
            let mut seen = Vec::new();
            for_each_promise_child(dp, &mut |i, c| seen.push((i, c)));
            assert_eq!(seen, vec![(PROMISE_VALUE_SLOT, vp)]);
        }
    }

    #[test]
    fn closure_reaction_env_is_an_edge_and_a_fn_pointer_is_not() {
        let mut d = pending();
        let mut env = pending();
        let (dp, envp) = (at(&mut d), at(&mut env));
        unsafe {
            *word(dp, RX_CB_OFF) = envp;
            assert!(!promise_has_edges(dp));
            *(dp as *mut u8).add(RX_KIND_OFF) = 1 | RX_CLOSURE;
            assert!(promise_has_edges(dp));
            let mut seen = Vec::new();
            for_each_promise_child(dp, &mut |i, c| seen.push((i, c)));
            assert_eq!(seen, vec![(RX_CB_SLOT, envp)]);
            promise_slot_clear(dp, RX_CB_SLOT);
            assert!(!promise_has_edges(dp));
        }
    }
}
