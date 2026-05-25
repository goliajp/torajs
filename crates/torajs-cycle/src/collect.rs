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

use crate::arr::{arr_len_of, arr_slot_at, arr_slot_clear};
use crate::buffer;
use crate::layout::{
    COLOR_BLACK, COLOR_GRAY, COLOR_PURPLE, COLOR_WHITE, FLAG_BUFFERED, FLAG_STATIC_LITERAL,
    HeapHeader, color_of, has_walkable_children, is_class_obj, layout_for_class_obj, set_color,
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
}

/// Mark phase — Bacon & Rajan's "MarkGray". Recursively descend from
/// `p`, color reachable children GRAY + trial-decrement their rc.
///
/// # Safety
/// `p` must satisfy `has_walkable_children`. The walk reads class
/// layouts / arr slots — caller must guarantee the heap is
/// well-formed.
unsafe fn mark_gray(p: *mut c_void) {
    if !unsafe { has_walkable_children(p) } {
        return;
    }
    let h = p as *mut HeapHeader;
    if color_of(h) == COLOR_GRAY {
        return;
    }
    unsafe { set_color(h, COLOR_GRAY) };
    if unsafe { is_class_obj(p) } {
        let lay = unsafe { layout_for_class_obj(p) };
        let n_children = unsafe { (*lay).n_children };
        for i in 0..n_children as usize {
            let off = unsafe { *(*lay).child_offsets.add(i) };
            let child = unsafe { *((p as *mut u8).add(off as usize) as *mut *mut c_void) };
            if !child.is_null() && unsafe { has_walkable_children(child) } {
                let ch = child as *mut HeapHeader;
                if unsafe { (*ch).flags } & FLAG_STATIC_LITERAL == 0 {
                    unsafe { (*ch).refcount -= 1 };
                }
                unsafe { mark_gray(child) };
            }
        }
    } else {
        // TAG_ARR
        let n = unsafe { arr_len_of(p) };
        for i in 0..n {
            let child = unsafe { arr_slot_at(p, i) };
            if !child.is_null() && unsafe { has_walkable_children(child) } {
                let ch = child as *mut HeapHeader;
                if unsafe { (*ch).flags } & FLAG_STATIC_LITERAL == 0 {
                    unsafe { (*ch).refcount -= 1 };
                }
                unsafe { mark_gray(child) };
            }
        }
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
        if unsafe { is_class_obj(p) } {
            let lay = unsafe { layout_for_class_obj(p) };
            let n_children = unsafe { (*lay).n_children };
            for i in 0..n_children as usize {
                let off = unsafe { *(*lay).child_offsets.add(i) };
                let child = unsafe { *((p as *mut u8).add(off as usize) as *mut *mut c_void) };
                if !child.is_null() {
                    unsafe { scan(child) };
                }
            }
        } else {
            // TAG_ARR
            let n = unsafe { arr_len_of(p) };
            for i in 0..n {
                let child = unsafe { arr_slot_at(p, i) };
                if !child.is_null() {
                    unsafe { scan(child) };
                }
            }
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
    if unsafe { is_class_obj(p) } {
        let lay = unsafe { layout_for_class_obj(p) };
        let n_children = unsafe { (*lay).n_children };
        for i in 0..n_children as usize {
            let off = unsafe { *(*lay).child_offsets.add(i) };
            let child = unsafe { *((p as *mut u8).add(off as usize) as *mut *mut c_void) };
            if !child.is_null() && unsafe { has_walkable_children(child) } {
                let ch = child as *mut HeapHeader;
                if unsafe { (*ch).flags } & FLAG_STATIC_LITERAL == 0 {
                    unsafe { (*ch).refcount += 1 };
                }
                if color_of(ch) != COLOR_BLACK {
                    unsafe { scan_black(child) };
                }
            }
        }
    } else {
        // TAG_ARR
        let n = unsafe { arr_len_of(p) };
        for i in 0..n {
            let child = unsafe { arr_slot_at(p, i) };
            if !child.is_null() && unsafe { has_walkable_children(child) } {
                let ch = child as *mut HeapHeader;
                if unsafe { (*ch).flags } & FLAG_STATIC_LITERAL == 0 {
                    unsafe { (*ch).refcount += 1 };
                }
                if color_of(ch) != COLOR_BLACK {
                    unsafe { scan_black(child) };
                }
            }
        }
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
        let n_children = unsafe { (*lay).n_children };
        // First sweep: recurse into WHITE children + zero the slot.
        for i in 0..n_children as usize {
            let off = unsafe { *(*lay).child_offsets.add(i) };
            let slot = unsafe { (p as *mut u8).add(off as usize) as *mut *mut c_void };
            let child = unsafe { *slot };
            if !child.is_null() && unsafe { has_walkable_children(child) } {
                let ch = child as *mut HeapHeader;
                if color_of(ch) == COLOR_WHITE {
                    unsafe {
                        *slot = core::ptr::null_mut();
                        collect_white(child);
                    }
                }
            }
        }
        // Second sweep: drop surviving (non-cycle) children via the
        // universal drop dispatch.
        for i in 0..n_children as usize {
            let off = unsafe { *(*lay).child_offsets.add(i) };
            let child = unsafe { *((p as *mut u8).add(off as usize) as *mut *mut c_void) };
            if !child.is_null() {
                unsafe { __torajs_value_drop_heap(child) };
            }
        }
    } else {
        // TAG_ARR
        let n = unsafe { arr_len_of(p) };
        for i in 0..n {
            let child = unsafe { arr_slot_at(p, i) };
            if !child.is_null() && unsafe { has_walkable_children(child) } {
                let ch = child as *mut HeapHeader;
                if color_of(ch) == COLOR_WHITE {
                    unsafe {
                        arr_slot_clear(p, i);
                        collect_white(child);
                    }
                }
            }
        }
        for i in 0..n {
            let child = unsafe { arr_slot_at(p, i) };
            if !child.is_null() {
                unsafe { __torajs_value_drop_heap(child) };
            }
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

    buffer::reset_len();
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
    unsafe { __torajs_cycle_collect() };
}
