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

use crate::arr::{ARR_PROPS_OFF, arr_child_at, arr_len_of, arr_slot_clear};
use crate::buffer;
use crate::closure_walk::{closure_trace_fn, trace_clear_tramp, trace_visit_tramp};
use crate::dynobj::{dynobj_child_at, dynobj_entries_len, dynobj_value_slot_clear};
use crate::layout::{
    CLASS_LAYOUT_FLAG_NAMED, CLOSURE_PROPS_OFF, COLOR_BLACK, COLOR_GRAY, COLOR_PURPLE, COLOR_WHITE,
    FLAG_BUFFERED, FLAG_STATIC_LITERAL, HeapHeader, OBJ_PROPS_OFF, TAG_BOOLEAN_WRAPPER,
    TAG_CLOSURE, TAG_DYNOBJ, TAG_NUMBER_WRAPPER, TAG_STRING_WRAPPER, WRAPPER_PROPS_OFF,
    arr_elems_walkable, color_of, has_walkable_children, is_class_obj, layout_for_class_obj,
    nan_box_is_cell_like, set_color,
};

/// Sentinel child index for a cell's out-of-band expando / props
/// slot (Arr +24, wrapper +16) — element indices are dense from 0,
/// so MAX can't collide.
const PROPS_SLOT_INDEX: u64 = u64::MAX;

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
pub(crate) unsafe fn for_each_child(p: *mut c_void, mut f: impl FnMut(u64, *mut c_void)) {
    if unsafe { is_class_obj(p) } {
        let lay = unsafe { layout_for_class_obj(p) };
        let n_children = unsafe { (*lay).n_children };
        for i in 0..n_children as u64 {
            let off = unsafe { *(*lay).child_offsets.add(i as usize) };
            let child = unsafe { *((p as *mut u8).add(off as usize) as *mut *mut c_void) };
            // An `any`-typed field holds NaN-box bits, not a pointer,
            // whenever it carries an immediate — `undefined` is
            // `0x…0a`, and dereferencing that reads address 10. Every
            // other shape filters immediates where it produces them
            // (`dynobj_child_at`, `arr_child_at`, the closure trace
            // trampoline); this arm answered them raw, so a consumer
            // without its own gate crashed. `collect_white`'s first
            // sweep has one (`has_walkable_children`) and its second
            // does not, which is where it landed.
            if !child.is_null() && nan_box_is_cell_like(child) {
                f(i, child);
            }
        }
        // +24 expando props dict (RFC 20260714-struct-dynamic-props
        // blade 1) — same fixed slot Closure / Arr / wrapper carry.
        // An `obj.x = <cycle>` expando is a cycle child like any
        // layout field.
        let props = unsafe { *((p as *const u8).add(OBJ_PROPS_OFF) as *const *mut c_void) };
        if !props.is_null() {
            f(PROPS_SLOT_INDEX, props);
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
    } else if unsafe { (*(p as *const HeapHeader)).type_tag } == TAG_CLOSURE {
        // Closure env (RFC 20260717 knife 3) — capture slots via the
        // knife-2 trace_fn (0 = no traceable captures), plus the +24
        // expando props dict every closure shares at a fixed offset.
        if let Some(trace) = unsafe { closure_trace_fn(p) } {
            let mut dyn_f: &mut dyn FnMut(u64, *mut c_void) = &mut f;
            unsafe {
                trace(
                    p,
                    trace_visit_tramp,
                    &mut dyn_f as *mut &mut dyn FnMut(u64, *mut c_void) as *mut c_void,
                )
            };
        }
        let props = unsafe { *((p as *const u8).add(CLOSURE_PROPS_OFF) as *const *mut c_void) };
        if !props.is_null() {
            f(PROPS_SLOT_INDEX, props);
        }
    } else if matches!(
        unsafe { (*(p as *const HeapHeader)).type_tag },
        TAG_NUMBER_WRAPPER | TAG_STRING_WRAPPER | TAG_BOOLEAN_WRAPPER
    ) {
        // Wrapper — single walkable child: the +16 expando props
        // dict (blade 3). The StringWrapper inner Str cell is a
        // leaf, handled by the teardown, never walked.
        let props = unsafe { *((p as *const u8).add(WRAPPER_PROPS_OFF) as *const *mut c_void) };
        if !props.is_null() {
            f(PROPS_SLOT_INDEX, props);
        }
    } else if let Some(off) =
        crate::layout_bag::bag_only_props_off(unsafe { (*(p as *const HeapHeader)).type_tag })
    {
        // The bag-only shapes (Map / Set / Date / RegExp / Promise /
        // buffer family / the three iterator cells) — one walkable
        // child, the lazy expando dict at the shape's own offset.
        // Their internal state (entry table, compiled program, buffer
        // data, iteration source) is the destructor's, which
        // `defer::finalize_all` runs on the corpse.
        let props = unsafe { *((p as *const u8).add(off) as *const *mut c_void) };
        if !props.is_null() {
            f(PROPS_SLOT_INDEX, props);
        }
    } else {
        // TAG_ARR — element slots only when the kind guarantees
        // walkable bits (a scalar-kind array reaches here for its
        // expando alone), plus the +24 expando props dict (blade 3).
        if arr_elems_walkable(unsafe { &*(p as *const HeapHeader) }) {
            let n = unsafe { arr_len_of(p) };
            for i in 0..n {
                let child = unsafe { arr_child_at(p, i) };
                if !child.is_null() {
                    f(i, child);
                }
            }
        }
        let props = unsafe { *((p as *const u8).add(ARR_PROPS_OFF) as *const *mut c_void) };
        if !props.is_null() {
            f(PROPS_SLOT_INDEX, props);
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
pub(crate) unsafe fn clear_child_slot(p: *mut c_void, i: u64) {
    if unsafe { is_class_obj(p) } {
        // The props sentinel MUST be tested before indexing
        // child_offsets — `child_offsets.add(u64::MAX)` is an OOB
        // read followed by a wild write (the closure arm's ordering).
        if i == PROPS_SLOT_INDEX {
            unsafe {
                *((p as *mut u8).add(OBJ_PROPS_OFF) as *mut *mut c_void) = core::ptr::null_mut()
            };
            return;
        }
        let lay = unsafe { layout_for_class_obj(p) };
        let off = unsafe { *(*lay).child_offsets.add(i as usize) };
        unsafe { *((p as *mut u8).add(off as usize) as *mut *mut c_void) = core::ptr::null_mut() };
    } else if unsafe { (*(p as *const HeapHeader)).type_tag } == TAG_DYNOBJ {
        unsafe { dynobj_value_slot_clear(p, i) };
    } else if unsafe { (*(p as *const HeapHeader)).type_tag } == TAG_CLOSURE {
        if i == PROPS_SLOT_INDEX {
            unsafe {
                *((p as *mut u8).add(CLOSURE_PROPS_OFF) as *mut *mut c_void) = core::ptr::null_mut()
            };
        } else if let Some(trace) = unsafe { closure_trace_fn(p) } {
            // Only the trace body knows which address slot `i` maps
            // to (env slot vs capture-box interior) — replay it with
            // the clear trampoline. O(caps) on the cold collect path.
            let target: u64 = i;
            unsafe { trace(p, trace_clear_tramp, &target as *const u64 as *mut c_void) };
        }
    } else if i == PROPS_SLOT_INDEX {
        // Wrapper +16 / bag-shape own offset / Arr +24 expando slot.
        let tag = unsafe { (*(p as *const HeapHeader)).type_tag };
        let off = if matches!(
            tag,
            TAG_NUMBER_WRAPPER | TAG_STRING_WRAPPER | TAG_BOOLEAN_WRAPPER
        ) {
            WRAPPER_PROPS_OFF
        } else {
            crate::layout_bag::bag_only_props_off(tag).unwrap_or(ARR_PROPS_OFF)
        };
        unsafe { *((p as *mut u8).add(off) as *mut *mut c_void) = core::ptr::null_mut() };
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
    // Second sweep (non-closure shapes): drop surviving NON-WALKABLE
    // children via the universal drop dispatch — shapes mark_gray
    // never trial-decremented (Str / BigInt / Promise / Symbol …), so
    // this corpse's +1 on them is still outstanding. Walkable
    // children are skipped wholesale: mark trial-decremented every
    // walkable edge out of this (then-GRAY) parent, and scan_black
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
    if unsafe { (*(p as *const HeapHeader)).type_tag } != TAG_CLOSURE {
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

    // Collect phase: sweep every WHITE node + defer its free.
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

    // Free step — before the compact so re-buffer pushes from
    // closure drop_fns land in the kept region.
    unsafe { crate::defer::finalize_all() };

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
