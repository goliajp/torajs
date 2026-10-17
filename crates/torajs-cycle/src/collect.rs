//! Bacon & Rajan trial-deletion algorithm — the three phases
//! (mark / scan / collect) plus the public `gc()` entry point and
//! the codegen-injected end-of-main drain.
//!
//! Algorithm reference: D. F. Bacon & V. T. Rajan, "Concurrent Cycle
//! Collection in Reference Counted Systems" (ECOOP 2001) — the same
//! approach Python's `gc` module + CPython's cyclic-garbage finder
//! use.
//!
//! Port of `runtime_cycle.c`'s mark/scan/collect section (P4.4,
//! 2026-05-24). 1:1 port — same `visit_children` shape, which now
//! lives beside this file as [`children`](crate::children): the
//! per-shape layout knowledge the three phases below keep asking
//! for, split out so a new cyclic shape is an arm there rather than
//! an edit here.
//!
//! ## Phase ordering
//!
//! `cycle_collect` runs three sequential passes over the buffer:
//!
//! 1. **Mark** — descend from each PURPLE root, color reachable
//!    children GRAY + trial-decrement their rc by 1. After the pass,
//!    any node whose rc is 0 is a confirmed cycle (no external refs).
//! 2. **Scan** — for each GRAY node: rc > 0 means externally
//!    reachable → the BLACK `repaint` restores the rc + recolors BLACK
//!    transitively. rc == 0 → recolor WHITE + descend into children.
//! 3. **Collect** — every WHITE node is confirmed garbage. Descend
//!    into WHITE children (clearing the slot to break the cycle),
//!    then drop non-cycle children normally, then `free()` the block.
//!
//! Every descent runs on an explicit [`WorkStack`], not the machine stack.

use core::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::buffer;
use crate::cell_free::free_parked;
use crate::children::{clear_child_slot, for_each_child};
use crate::layout::{
    CLASS_LAYOUT_FLAG_NAMED, COLOR_BLACK, COLOR_GRAY, COLOR_PURPLE, COLOR_WHITE, FLAG_BUFFERED,
    FLAG_STATIC_LITERAL, HeapHeader, TAG_CLOSURE, color_of, has_walkable_children, is_class_obj,
    layout_for_class_obj, set_color,
};
use crate::work_stack::WorkStack;

/// Set while a collect runs. A drop inside pass 3 that pushes the
/// 1024th candidate would otherwise start a nested collect, and the
/// nested root walk could not tell a black survivor being dismantled
/// from a block that finished dying while buffered. `unbuffer` also
/// reads it: the buffer keeps its length while a collect walks it.
static IN_COLLECT: AtomicBool = AtomicBool::new(false);

/// True while a collect runs.
#[inline]
pub(crate) fn in_collect() -> bool {
    IN_COLLECT.load(Ordering::Relaxed)
}

unsafe extern "C" {
    /// Universal-drop dispatcher in runtime_str.c — type-tag-keyed
    /// per-flavor cleanup (Str / Arr / Obj / Map / WeakRef / ...).
    /// Called by `collect_white` to drop surviving (non-cycle)
    /// children with their type-specific dec paths.
    fn __torajs_value_drop_heap(p: *mut c_void);

    /// torajs-weak — clear WeakRefs pointing at a dying target
    /// (chunk 620, mirror of `obj_drop_rc`'s named-class hook: a
    /// cycle-collected obj previously left its WeakRefs dangling —
    /// deref after the collect returned freed memory).
    fn __torajs_weakref_target_dying(target: *mut c_void);
}

/// Tag on a [`collect_white`] entry asking for the cell's closing half
/// once the WHITE cells below it are swept (cells are word-aligned).
const AFTER_CHILDREN: usize = 1;

/// Mark phase — Bacon & Rajan's "MarkGray". Descend from `root`,
/// color reachable children GRAY + trial-decrement their rc, once
/// per edge out of each cell that turns GRAY.
///
/// # Safety
/// Same as [`for_each_child`].
unsafe fn mark_gray(root: *mut c_void, st: &mut WorkStack) {
    if unsafe { has_walkable_children(root) } {
        unsafe { repaint(root, st, COLOR_GRAY) };
    }
}

/// Scan phase — for each GRAY node decide WHITE (cycle garbage) or
/// hand off to `repaint` BLACK (externally reachable, restore rc +
/// recolor BLACK). The outcome does not depend on visit order: a cell
/// ends BLACK exactly when it is reachable from a GRAY cell whose
/// trial rc stayed positive, because the BLACK repaint overrides WHITE.
///
/// # Safety
/// Same as `mark_gray`.
#[inline(never)]
unsafe fn scan(root: *mut c_void, st: &mut WorkStack) {
    if !unsafe { has_walkable_children(root) } || unsafe { color_of(root.cast()) } != COLOR_GRAY {
        return;
    }
    let mut p = root;
    loop {
        let h = p as *mut HeapHeader;
        if unsafe { has_walkable_children(p) } && unsafe { color_of(h) } == COLOR_GRAY {
            if unsafe { (*h).refcount } > 0 {
                // externally referenced — recolor BLACK and restore the
                // trial rc decrement across all gray descendants
                unsafe { repaint(p, st, COLOR_BLACK) };
            } else {
                unsafe { set_color(h, COLOR_WHITE) };
                unsafe {
                    for_each_child(p, |_, child| st.push(child as usize));
                }
            }
        }
        let Some(v) = st.pop() else { break };
        p = v as *mut c_void;
    }
}

/// The walk MarkGray and ScanBlack share: recolor `root` and
/// every walkable cell reachable from it that is not `color` yet, and
/// move the rc of each walkable child of a recolored cell by one — down
/// for GRAY (the trial deletion), back up for BLACK (its undo, counting
/// each BLACK cell as live). Runs to completion above the entries
/// already on the stack.
///
/// # Safety
/// Same as `mark_gray`; `root` is walkable and not `color`.
#[inline(never)]
unsafe fn repaint(root: *mut c_void, st: &mut WorkStack, color: u16) {
    let black = color == COLOR_BLACK;
    // only walkable cells are pushed
    let base = st.len();
    let mut p = root;
    loop {
        let h = p as *mut HeapHeader;
        if unsafe { color_of(h) } != color {
            unsafe { set_color(h, color) };
            if black {
                buffer::note_live();
            }
            unsafe {
                for_each_child(p, |_, child| {
                    if has_walkable_children(child) {
                        let ch = child as *mut HeapHeader;
                        if (*ch).flags & FLAG_STATIC_LITERAL == 0 {
                            if black {
                                (*ch).refcount += 1;
                            } else {
                                (*ch).refcount -= 1;
                            }
                        }
                        if color_of(ch) != color {
                            st.push(child as usize);
                        }
                    }
                });
            }
        }
        let Some(v) = st.pop_above(base) else { break };
        p = v as *mut c_void;
    }
}

/// Collect phase — every WHITE node is confirmed cycle garbage with
/// no external refs. Cells are swept in depth-first recursion order:
/// a cell's opening half (recolor, weak notify, first sweep) before
/// the WHITE cells below it, in slot order, and its closing half
/// (second sweep, deferred free) after them.
///
/// # Safety
/// Same as `mark_gray`; `root` is a WHITE cell. A WHITE
/// child already swept through another parent is skipped when popped
/// (a child collected twice would double-free).
#[inline(never)]
unsafe fn collect_white(root: *mut c_void, st: &mut WorkStack) {
    if !unsafe { has_walkable_children(root) } {
        return;
    }
    let mut v = root as usize;
    loop {
        let p = (v & !AFTER_CHILDREN) as *mut c_void;
        if v & AFTER_CHILDREN != 0 || unsafe { collect_white_open(p, st) } {
            unsafe { collect_white_close(p) };
        }
        let Some(next) = st.pop() else { break };
        v = next;
    }
}

/// The opening half of sweeping a cell still WHITE. True when it has
/// no WHITE children and closes at once; else its closing entry goes under them.
///
/// # Safety
/// Same as `collect_white`.
#[inline]
unsafe fn collect_white_open(p: *mut c_void, st: &mut WorkStack) -> bool {
    let h = p as *mut HeapHeader;
    if unsafe { color_of(h) } != COLOR_WHITE {
        return false;
    }
    // Recolor BLACK + clear BUFFERED first so re-entry from a
    // sibling cycle doesn't double-collect this node.
    unsafe {
        set_color(h, COLOR_BLACK);
        (*h).flags &= !FLAG_BUFFERED;
    }
    if unsafe { is_class_obj(p) } {
        let lay = unsafe { layout_for_class_obj(p) };
        // Named-class instances can be WeakRef / WeakMap-key
        // targets — notify before teardown, exactly like the
        // normal-drop path (chunk 620; anonymous structs are
        // never weak targets on either path).
        if unsafe { (*lay).flags } & CLASS_LAYOUT_FLAG_NAMED != 0 {
            unsafe { __torajs_weakref_target_dying(p) };
        }
    }
    // First sweep: queue WHITE children + zero the slot.
    let base = st.len();
    unsafe {
        for_each_child(p, |i, child| {
            if has_walkable_children(child) {
                let ch = child as *mut HeapHeader;
                if color_of(ch) == COLOR_WHITE {
                    clear_child_slot(p, i);
                    st.push(child as usize);
                }
            }
        });
    }
    if st.len() == base {
        return true;
    }
    // c1..ck then the closing entry, reversed: c1 pops first, it last
    st.push(p as usize | AFTER_CHILDREN);
    st.reverse_from(base);
    false
}

/// The closing half of sweeping a cell, once the cells below it are swept.
///
/// # Safety
/// `p` is a cell `collect_white` opened in this collect.
unsafe fn collect_white_close(p: *mut c_void) {
    // Second sweep (non-closure shapes): drop surviving NON-WALKABLE
    // children via the universal drop dispatch — shapes mark_gray
    // never trial-decremented (Str / BigInt / Promise / Symbol …), so
    // this corpse's +1 on them is still outstanding. Walkable
    // children are skipped wholesale: mark trial-decremented every
    // walkable edge out of this (then-GRAY) parent, and ScanBlack
    // only restores edges out of BLACK parents — for a child that
    // survived as BLACK, the unrestored trial-dec IS this dying
    // parent's release, and dropping it here charged the same edge
    // twice (two WHITE class-proto parents dec'd %Object.prototype%
    // from its BLACK-restored rc straight through zero — the class
    // census family). A fellow cycle member is equally out of bounds:
    // re-dropping it underflows the rc and re-buffers a corpse
    // (chunk 614: the l8 obj↔arr probe crashed here). The rc > 0
    // gate stays because a corpse's cleared slots can flip
    // `has_walkable_children` (closure props / arr expando) to false
    // after the fact. Closures skip this — their synthesized drop_fn
    // owns the per-slot release semantics (capture-box rc vs owned
    // cell vs Any NaN-box vs Substr view) and runs in the deferred
    // Free step, with the walkable slots pre-cleared by pass A.
    //
    // The bag-only shapes skip it for the same reason, and with the
    // entry table in the walk it now matters: `defer` pass B hands
    // each of them back to its own destructor, and a Map's
    // destructor releases every heap-tagged entry it still holds. A
    // non-walkable key — a Str, a BigInt — would be released here
    // AND there, the double release the closure carve-out exists to
    // prevent. Pass A clears exactly the slots this collect already
    // accounted for; whatever is left is the destructor's.
    let tag = unsafe { (*(p as *const HeapHeader)).type_tag };
    if tag != TAG_CLOSURE && !crate::layout_bag::corpse_takes_own_destructor(tag) {
        unsafe {
            for_each_child(p, |_, child| {
                if (*(child as *const HeapHeader)).refcount > 0 && !has_walkable_children(child) {
                    __torajs_value_drop_heap(child);
                }
            });
        }
    }
    // The free itself is DEFERRED to `defer::finalize_all` after the
    // collect pass (Bacon-Rajan's separate Free step). Freeing here
    // let a later WHITE parent's sweep read this block's freed
    // header — mmalloc's free-list pointer overwrites the refcount
    // word, faking rc > 0, so the corpse got re-dropped and
    // re-buffered as a stale root (rotation-165 setops SIGSEGV).
    crate::defer::defer_push(p);
}

/// Public `gc()` user trigger, and the auto-collect path: the three
/// phases over the current buffer contents. The `len == 0` early-out
/// keeps the no-op case cheap.
///
/// # Safety
/// Every pointer in the root buffer is a cell that has not been freed; callers keep that by unbuffering cells before freeing them.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_cycle_collect() {
    if buffer::len() == 0 || IN_COLLECT.swap(true, Ordering::Relaxed) {
        return;
    }
    #[cfg(torajs_census)]
    let census_start = census::now();
    // Entries appended DURING the phases (pass-3 drops re-buffer
    // surviving children) are next-collect candidates — the compact
    // below keeps them instead of blanket-resetting the length.
    let processed = buffer::len();
    let mut st = WorkStack::take();

    // Mark phase: descend from each buffered root. A block whose rc
    // is already 0 is either dead and coloured black by the drop that
    // dismantled it (free it here, its children are long released) or
    // still being dismantled by a drop further up the stack (leave it;
    // that drop frees it once pass 3 has cleared its buffered bit).
    buffer::for_each_with_index(|i, p| {
        if p.is_null() {
            return;
        }
        let h = p as *mut HeapHeader;
        if unsafe { (*h).flags } & FLAG_STATIC_LITERAL != 0 {
            return;
        }
        if unsafe { (*h).refcount } == 0 {
            if unsafe { color_of(h) } == COLOR_BLACK {
                buffer::clear_slot(i);
                // a class instance or closure env that died while
                // buffered, each carrying its own size record
                #[cfg(torajs_census)]
                census::parked(unsafe { (*h).type_tag } == TAG_CLOSURE);
                unsafe { free_parked(p) };
            }
            return;
        }
        if unsafe { color_of(h) } == COLOR_PURPLE {
            unsafe { mark_gray(p, &mut st) };
        }
        // If already gray (visited transitively in another root's
        // walk), nothing to do — but keep in buffer for scan phase.
    });

    // Scan phase: distinguish WHITE garbage from BLACK-restore.
    buffer::for_each(|p| unsafe { scan(p, &mut st) });

    // Collect phase: sweep every WHITE node + defer its free. Class
    // instances only die in `finalize_all`, after this pass has
    // cleared every buffered bit, so nothing in this range turns into
    // a black corpse here; one that dies buffered during finalize sits
    // in the kept region for the next root walk.
    buffer::for_each_with_index(|_i, p| {
        if p.is_null() {
            return;
        }
        let h = p as *mut HeapHeader;
        unsafe { (*h).flags &= !FLAG_BUFFERED };
        if unsafe { color_of(h) } == COLOR_WHITE {
            unsafe { collect_white(p, &mut st) };
        }
    });

    // Free step — before the compact so re-buffer pushes from
    // closure drop_fns land in the kept region.
    unsafe { crate::defer::finalize_all() };
    drop(st);

    buffer::compact_from(processed);
    buffer::retune_threshold();
    #[cfg(torajs_census)]
    census::record(processed as u64, census_start);
    IN_COLLECT.store(false, Ordering::Relaxed);
}

/// Main-exit drain. Public symbol the codegen wires into the
/// synthesized main as a final tail call, after every top-level
/// scope's drops have run. Drains any cycle roots still in the
/// buffer so leaked cycles don't survive program teardown.
///
/// Explicit-call rather than `#[ctor::dtor]` mirrors the C
/// rationale: a destructor runs after libc's atexit pipeline,
/// which on macOS has already torn down some thread-local state —
/// calls into the runtime that touch malloc/free can crash.
///
/// # Safety
/// Same as [`__torajs_cycle_collect`]; call once, after every top-level drop has run.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_cycle_at_exit_drain() {
    // Pass-3 drops can re-buffer surviving candidates (kept by the
    // compact) — loop until quiescent. Terminates: entries appended
    // during a round require frees in that round, and the heap is
    // finite; a round that frees nothing appends nothing.
    while buffer::len() > 0 {
        unsafe { __torajs_cycle_collect() };
    }
}

#[cfg(torajs_census)]
mod census;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{ARR_ELEM_KIND_SHIFT, ARR_KIND_HEAP, TAG_ARR};

    // A one-slot heap-element array cell, laid out the way `arr.rs`
    // reads it: len +8, cap/head +16, props +24, data pointer +32.
    #[repr(C)]
    struct Cell {
        hdr: HeapHeader,
        len: u64,
        cap_head: u64,
        props: u64,
        data: *mut u64,
        slot: u64,
    }

    fn cells(n: usize) -> Vec<Cell> {
        let mut v: Vec<Cell> = (0..n)
            .map(|_| Cell {
                hdr: HeapHeader {
                    refcount: 0,
                    type_tag: TAG_ARR,
                    flags: (ARR_KIND_HEAP << ARR_ELEM_KIND_SHIFT) | COLOR_PURPLE,
                },
                len: 1,
                cap_head: 1,
                props: 0,
                data: core::ptr::null_mut(),
                slot: 0,
            })
            .collect();
        for c in v.iter_mut() {
            c.data = &raw mut c.slot;
        }
        v
    }

    fn ptr(c: &Cell) -> *mut c_void {
        c as *const Cell as *mut c_void
    }

    // cell i holds cell i+1, the last holds the first; each cell is
    // referenced once, by its predecessor
    fn ring(n: usize) -> Vec<Cell> {
        let mut v = cells(n);
        for i in 0..n {
            let next = ptr(&v[(i + 1) % n]) as u64;
            v[i].slot = next;
            v[i].hdr.refcount = 1;
        }
        v
    }

    // deeper than any machine-stack walk survives on a test thread
    const DEEP: usize = 300_000;

    #[test]
    fn unreferenced_deep_ring_turns_white_and_is_swept_once_per_cell() {
        let v = ring(DEEP);
        let root = ptr(&v[0]);
        let mut st = WorkStack::take();
        unsafe {
            mark_gray(root, &mut st);
            scan(root, &mut st);
        }
        assert!(v.iter().all(|c| c.hdr.refcount == 0));
        assert!(v.iter().all(|c| unsafe { color_of(&c.hdr) } == COLOR_WHITE));
        unsafe { collect_white(root, &mut st) };
        assert_eq!(st.len(), 0);
        // every edge into a WHITE cell is cleared; the closing edge
        // reaches the root after it was opened, and stays
        assert!(v[..DEEP - 1].iter().all(|c| c.slot == 0));
        assert_eq!(v[DEEP - 1].slot, root as u64);
        assert!(v.iter().all(|c| unsafe { color_of(&c.hdr) } == COLOR_BLACK));
        // post-order, the way the recursive sweep deferred them
        let swept = crate::defer::take_for_test();
        assert_eq!(swept.len(), DEEP);
        assert_eq!(swept[0], ptr(&v[DEEP - 1]));
        assert_eq!(swept[DEEP - 1], root);
    }

    #[test]
    fn externally_held_deep_ring_survives_with_its_counts_restored() {
        let mut v = ring(DEEP);
        v[DEEP / 2].hdr.refcount = 2;
        let root = ptr(&v[0]);
        let mut st = WorkStack::take();
        unsafe {
            mark_gray(root, &mut st);
            scan(root, &mut st);
        }
        assert_eq!(st.len(), 0);
        for (i, c) in v.iter().enumerate() {
            assert_eq!(c.hdr.refcount, if i == DEEP / 2 { 2 } else { 1 });
            assert_eq!(unsafe { color_of(&c.hdr) }, COLOR_BLACK);
        }
    }

    #[test]
    fn a_cell_reached_from_two_white_parents_is_swept_once() {
        // a -> b, b -> d, c -> d, d -> a, plus c held by a's props
        // slot: d is reached from both b and c
        let mut v = cells(4);
        let [a, b, c, d] = [0, 1, 2, 3].map(|i| ptr(&v[i]));
        v[0].slot = b as u64;
        v[0].props = c as u64;
        v[1].slot = d as u64;
        v[2].slot = d as u64;
        v[3].slot = a as u64;
        for (i, rc) in [1, 1, 1, 2].into_iter().enumerate() {
            v[i].hdr.refcount = rc;
        }
        let mut st = WorkStack::take();
        unsafe {
            mark_gray(a, &mut st);
            scan(a, &mut st);
            collect_white(a, &mut st);
        }
        let swept = crate::defer::take_for_test();
        assert_eq!(swept, vec![d, b, c, a]);
    }
}
