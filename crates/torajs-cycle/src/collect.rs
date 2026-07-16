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
//! 2026-05-24). 1:1 port — same `visit_children` shape, same per-
//! tag dispatch between TAG_OBJ (walk via class_layouts) and TAG_ARR
//! (walk via slot iteration).
//!
//! ## Phase ordering
//!
//! `cycle_collect` runs three sequential passes over the buffer:
//!
//! 1. **Mark** — descend from each PURPLE root, color reachable
//!    children GRAY + trial-decrement their rc by 1. After the pass,
//!    any node whose rc is 0 is a confirmed cycle (no external refs).
//! 2. **Scan** — for each GRAY node: rc > 0 means externally
//!    reachable → `scan_black` restores the rc + recolors BLACK
//!    transitively. rc == 0 → recolor WHITE + recurse into children.
//! 3. **Collect** — every WHITE node is confirmed garbage. Recurse
//!    into WHITE children (clearing the slot to break the cycle),
//!    then drop non-cycle children normally, then `free()` the block.

use core::ffi::c_void;

use crate::arr::{ARR_PROPS_OFF, arr_child_at, arr_len_of, arr_slot_clear, arr_spilled_data};
use crate::buffer;
use crate::dynobj::{
    dynobj_block_bytes, dynobj_child_at, dynobj_entries_len, dynobj_key_at, dynobj_value_slot_clear,
};
use crate::layout::{
    CLASS_LAYOUT_FLAG_NAMED, COLOR_BLACK, COLOR_GRAY, COLOR_PURPLE, COLOR_WHITE, FLAG_BUFFERED,
    FLAG_STATIC_LITERAL, HeapHeader, TAG_DYNOBJ, color_of, has_walkable_children, is_class_obj,
    layout_for_class_obj, set_color,
};

unsafe extern "C" {
    /// torajs-mmalloc libc-compat free — v0.7-A2 step 6b finale. This
    /// free releases cross-crate heap (Arr / Map / Obj / DynObj — any
    /// cyclic-shape heap caught by the trial-deletion walk). Every
    /// allocating crate must already be on mmalloc before this cut
    /// fires; otherwise the free routes to the wrong allocator.
    #[link_name = "__torajs_libc_free"]
    fn free(p: *mut c_void);

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

    /// torajs-str — release a dynobj entry's key Str during the
    /// collect_white dict teardown (RFC 20260717 blade 2).
    fn __torajs_str_drop(s: *mut c_void);

    /// torajs-mmalloc sized free — the DynObj block is a sized
    /// `__torajs_calloc` allocation with NO libc-shim size header,
    /// so the unsized `__torajs_libc_free` above must not touch it
    /// (it reads a size word at `p - 8` that was never written).
    #[link_name = "__torajs_free"]
    fn free_sized(p: *mut c_void, size: usize);
}

/// Per-shape walkable-child slot enumeration — the ONLY place that
/// knows how each cyclic shape stores its children (class Obj =
/// layout child_offsets; Arr = element slots). Calls `f(i, child)`
/// for every non-null candidate slot; callers apply their own
/// `has_walkable_children` / color / rc gates. The slot index `i`
/// round-trips into [`clear_child_slot`].
///
/// # Safety
/// `p` must satisfy `has_walkable_children`. The walk reads class
/// layouts / arr slots — caller must guarantee the heap is
/// well-formed.
unsafe fn for_each_child(p: *mut c_void, mut f: impl FnMut(u64, *mut c_void)) {
    if unsafe { is_class_obj(p) } {
        let lay = unsafe { layout_for_class_obj(p) };
        let n_children = unsafe { (*lay).n_children };
        for i in 0..n_children as u64 {
            let off = unsafe { *(*lay).child_offsets.add(i as usize) };
            let child = unsafe { *((p as *mut u8).add(off as usize) as *mut *mut c_void) };
            if !child.is_null() {
                f(i, child);
            }
        }
    } else if unsafe { (*(p as *const HeapHeader)).type_tag } == TAG_DYNOBJ {
        // Dense entry values (holes / NaN-box immediates filter to
        // NULL inside dynobj_child_at).
        let n = unsafe { dynobj_entries_len(p) };
        for i in 0..n {
            let child = unsafe { dynobj_child_at(p, i) };
            if !child.is_null() {
                f(i, child);
            }
        }
    } else {
        // TAG_ARR
        let n = unsafe { arr_len_of(p) };
        for i in 0..n {
            let child = unsafe { arr_child_at(p, i) };
            if !child.is_null() {
                f(i, child);
            }
        }
    }
}

/// Zero child slot `i` of `p` — shape-dispatched, index semantics per
/// [`for_each_child`]. Used by `collect_white`'s first sweep to break
/// the cycle before recursing.
///
/// # Safety
/// Same as [`for_each_child`]; `i` must be an index `for_each_child`
/// yielded for this `p`.
unsafe fn clear_child_slot(p: *mut c_void, i: u64) {
    if unsafe { is_class_obj(p) } {
        let lay = unsafe { layout_for_class_obj(p) };
        let off = unsafe { *(*lay).child_offsets.add(i as usize) };
        unsafe { *((p as *mut u8).add(off as usize) as *mut *mut c_void) = core::ptr::null_mut() };
    } else if unsafe { (*(p as *const HeapHeader)).type_tag } == TAG_DYNOBJ {
        unsafe { dynobj_value_slot_clear(p, i) };
    } else {
        unsafe { arr_slot_clear(p, i) };
    }
}

/// Mark phase — Bacon & Rajan's "MarkGray". Recursively descend from
/// `p`, color reachable children GRAY + trial-decrement their rc.
///
/// # Safety
/// Same as [`for_each_child`].
unsafe fn mark_gray(p: *mut c_void) {
    if !unsafe { has_walkable_children(p) } {
        return;
    }
    let h = p as *mut HeapHeader;
    if color_of(h) == COLOR_GRAY {
        return;
    }
    unsafe { set_color(h, COLOR_GRAY) };
    unsafe {
        for_each_child(p, |_, child| {
            if has_walkable_children(child) {
                let ch = child as *mut HeapHeader;
                if (*ch).flags & FLAG_STATIC_LITERAL == 0 {
                    (*ch).refcount -= 1;
                }
                mark_gray(child);
            }
        });
    }
}

/// Scan phase — for each GRAY node decide WHITE (cycle garbage) or
/// hand off to `scan_black` (externally reachable, restore rc +
/// recolor BLACK).
///
/// # Safety
/// Same as `mark_gray`.
unsafe fn scan(p: *mut c_void) {
    if !unsafe { has_walkable_children(p) } {
        return;
    }
    let h = p as *mut HeapHeader;
    if color_of(h) != COLOR_GRAY {
        return;
    }
    if unsafe { (*h).refcount } > 0 {
        unsafe { scan_black(p) };
    } else {
        unsafe { set_color(h, COLOR_WHITE) };
        unsafe {
            for_each_child(p, |_, child| scan(child));
        }
    }
}

/// Externally referenced — recolor BLACK and restore the trial rc
/// decrement, transitively across all gray descendants.
///
/// # Safety
/// Same as `mark_gray`.
unsafe fn scan_black(p: *mut c_void) {
    if !unsafe { has_walkable_children(p) } {
        return;
    }
    let h = p as *mut HeapHeader;
    unsafe { set_color(h, COLOR_BLACK) };
    unsafe {
        for_each_child(p, |_, child| {
            if has_walkable_children(child) {
                let ch = child as *mut HeapHeader;
                if (*ch).flags & FLAG_STATIC_LITERAL == 0 {
                    (*ch).refcount += 1;
                }
                if color_of(ch) != COLOR_BLACK {
                    scan_black(child);
                }
            }
        });
    }
}

/// Collect phase — every WHITE node is confirmed cycle garbage with
/// no external refs. First pass recurses into WHITE children (zeroes
/// the slot so the recursion doesn't re-decrement); second pass
/// drops surviving non-cycle children via `value_drop_heap` (their
/// type-specific dec path). Then `free()` the block itself.
///
/// # Safety
/// Same as `mark_gray`. The slot-clear before recursion is what
/// keeps the algorithm tail-recursive-safe (a child being collected
/// twice would double-free).
unsafe fn collect_white(p: *mut c_void) {
    if !unsafe { has_walkable_children(p) } {
        return;
    }
    let h = p as *mut HeapHeader;
    if color_of(h) != COLOR_WHITE {
        return;
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
    // First sweep: recurse into WHITE children + zero the slot.
    unsafe {
        for_each_child(p, |i, child| {
            if has_walkable_children(child) {
                let ch = child as *mut HeapHeader;
                if color_of(ch) == COLOR_WHITE {
                    clear_child_slot(p, i);
                    collect_white(child);
                }
            }
        });
    }
    // Second sweep: drop surviving (non-cycle) children via the
    // universal drop dispatch. rc == 0 marks a cycle member being
    // freed by its own collect_white frame up the recursion (its
    // slot stayed set because it recolored BLACK on entry, so the
    // first sweep didn't clear it) — re-dropping it underflows
    // the rc and re-buffers a block that's about to be freed
    // (chunk 614: the l8 obj↔arr probe crashed here). Surviving
    // children always carry rc ≥ 1: non-walkable types are never
    // trial-decremented and externally-reachable walkables had
    // their rc restored by scan_black.
    unsafe {
        for_each_child(p, |_, child| {
            if (*(child as *const HeapHeader)).refcount > 0 {
                __torajs_value_drop_heap(child);
            }
        });
    }
    let tag = unsafe { (*(p as *const HeapHeader)).type_tag };
    if tag == TAG_DYNOBJ {
        // Dict teardown extras — every live entry still owns its key
        // Str (the sweeps only touched value slots); values were
        // either collected (slot cleared) or dropped by the second
        // sweep. Mirror __torajs_dynobj_drop's key walk, then free
        // the single block (index + entries are inline) through the
        // SIZED free — see the `free_sized` extern doc.
        let n = unsafe { dynobj_entries_len(p) };
        for i in 0..n {
            let key = unsafe { dynobj_key_at(p, i) };
            if !key.is_null() {
                unsafe { __torajs_str_drop(key) };
            }
        }
        unsafe { free_sized(p, dynobj_block_bytes(p)) };
        return;
    }
    if !unsafe { is_class_obj(p) } {
        // TAG_ARR teardown extras.
        // Mirror __torajs_arr_drop_any's teardown — release the
        // inline props-dynobj slot (a dynobj joined the walk in RFC
        // 20260717 blade 2, but the props slot is not an element
        // slot, so the trial-deletion walk can't have released it;
        // pre-fix a cycle-collected `arr.x = v` array leaked its
        // props).
        let props = unsafe { *((p as *const u8).add(ARR_PROPS_OFF) as *const *mut c_void) };
        if !props.is_null() {
            unsafe { __torajs_value_drop_heap(props) };
        }
        // B1 — a grown array spilled its slots to an independent
        // buffer; release it alongside the cell (mirror of
        // torajs-arr __torajs_arr_free's spill branch — pre-fix
        // every cycle-collected grown array leaked its buffer).
        let spill = unsafe { arr_spilled_data(p) };
        if !spill.is_null() {
            unsafe { free(spill as *mut c_void) };
        }
    }
    unsafe { free(p) };
}

/// Public `gc()` user trigger. Runs the three phases over the
/// current buffer contents, then resets the buffer length.
///
/// `cycle_buffer`'s auto-collect threshold also calls this path —
/// keep the early-out (`len == 0`) at the top so the no-op case
/// stays cheap.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_cycle_collect() {
    if buffer::len() == 0 {
        return;
    }
    // Entries appended DURING the phases (pass-3 drops re-buffer
    // surviving children) are next-collect candidates — the compact
    // below keeps them instead of blanket-resetting the length.
    let processed = buffer::len();

    // Mark phase: descend from each buffered root.
    buffer::for_each(|p| {
        let h = p as *mut HeapHeader;
        if color_of(h) == COLOR_PURPLE {
            unsafe { mark_gray(p) };
        }
        // If already gray (visited transitively in another root's
        // walk), nothing to do — but keep in buffer for scan phase.
    });

    // Scan phase: distinguish WHITE garbage from BLACK-restore.
    buffer::for_each(|p| unsafe { scan(p) });

    // Collect phase: free every WHITE node + its children.
    buffer::for_each_with_index(|_i, p| {
        if p.is_null() {
            return;
        }
        let h = p as *mut HeapHeader;
        unsafe { (*h).flags &= !FLAG_BUFFERED };
        if color_of(h) == COLOR_WHITE {
            unsafe { collect_white(p) };
        }
    });

    buffer::compact_from(processed);
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
