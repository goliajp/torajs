//! Buffer of potential cycle roots (PURPLE candidates).
//!
//! Port of the `g_buffer` + `__torajs_cycle_buffer` +
//! `__torajs_cycle_unbuffer` section of `runtime_cycle.c`. The
//! `rc_dec` / Obj-walk_blk hooks push candidates here when an rc
//! transition stays positive on a cyclic-shape type; the auto-collect
//! threshold + the explicit `gc()` trigger drain the buffer through
//! the trial-deletion algorithm (in `crate::collect`).
//!
//! ## `static mut` 替代 → `AtomicPtr`
//!
//! Same rationale as `crate::registry` / `torajs-arr::pool` —
//! tora runtime is single-threaded, but Rust 2024 deprecates raw
//! `static mut`. `AtomicPtr<*mut c_void>` + `AtomicU32` + `Relaxed`
//! compiles to identical loads/stores while keeping `&'static`
//! APIs sound + pre-paying the future multi-threaded story.

use core::ffi::c_void;
use core::ptr;
use std::sync::atomic::{AtomicPtr, AtomicU32, Ordering};

use crate::layout::{
    CLASS_LAYOUT_FLAG_NAMED, COLOR_PURPLE, FLAG_BUFFERED, HeapHeader, has_walkable_children,
    is_class_obj, layout_for_class_obj, nan_box_is_cell_like, set_color,
};

/// Floor of the buffer length at which `cycle_buffer` triggers a
/// synchronous collect before returning (`runtime_cycle.c`'s fixed
/// 1024). Keeps long-running programs that never call `gc()`
/// explicitly from leaking cycle roots unbounded.
pub const CYCLE_AUTO_COLLECT_THRESHOLD: u32 = 1024;

/// The live threshold. A collect walks everything its roots reach,
/// so with a fixed threshold a large live graph behind the roots is
/// re-walked every 1024 pushes, quadratic over a program that keeps
/// growing it. After each collect the threshold becomes a quarter of
/// the cells it walked and found live, never below the floor —
/// CPython's `long_lived_pending > long_lived_total / 4` rule — so
/// the walk is paid for by the pushes that precede it.
static G_THRESHOLD: AtomicU32 = AtomicU32::new(CYCLE_AUTO_COLLECT_THRESHOLD);

/// Cells the running collect has proven live (its BLACK `repaint` colours
/// each one once).
static G_LIVE: AtomicU32 = AtomicU32::new(0);

/// Count one cell a collect found externally reachable.
#[inline]
pub(crate) fn note_live() {
    G_LIVE.store(G_LIVE.load(Ordering::Relaxed) + 1, Ordering::Relaxed);
}

/// Set the next collect's threshold from this one's live count.
pub(crate) fn retune_threshold() {
    let live = G_LIVE.swap(0, Ordering::Relaxed);
    let next = (live / 4).max(CYCLE_AUTO_COLLECT_THRESHOLD);
    G_THRESHOLD.store(next, Ordering::Relaxed);
}

/// Pointer to the buffer's heap-allocated backing array. NULL until
/// first push; `realloc`'d to `g_buffer_cap` × `sizeof(*mut c_void)`
/// on growth.
static G_BUFFER: AtomicPtr<*mut c_void> = AtomicPtr::new(ptr::null_mut());

/// Number of valid entries in `G_BUFFER`. A slot is NULL when
/// `cycle_unbuffer` scrubbed it, or names a block whose rc reached 0
/// while it was buffered: that block stays allocated, coloured black,
/// until the next collect's root walk frees it.
static G_BUFFER_LEN: AtomicU32 = AtomicU32::new(0);

/// Allocated capacity. Doubles from 64 on growth.
static G_BUFFER_CAP: AtomicU32 = AtomicU32::new(0);

unsafe extern "C" {
    /// torajs-mmalloc libc-compat realloc — v0.7-A2 step 6b cutover.
    /// Closed-loop within cycle (this is cycle's own root buffer; the
    /// cross-crate `free` in collect.rs is the separate finale path).
    #[link_name = "__torajs_realloc"]
    fn realloc(p: *mut c_void, old_size: usize, new_size: usize) -> *mut c_void;
}

/// Append `p` to the buffer, reallocating + doubling capacity when
/// the current cap is reached. Initial cap = 64; doubles thereafter.
/// Single-threaded — callers don't need to lock.
fn buffer_push(p: *mut c_void) {
    let len = G_BUFFER_LEN.load(Ordering::Relaxed);
    let cap = G_BUFFER_CAP.load(Ordering::Relaxed);
    if len == cap {
        let new_cap = if cap == 0 { 64 } else { cap * 2 };
        let cur = G_BUFFER.load(Ordering::Relaxed);
        let elem = core::mem::size_of::<*mut c_void>();
        let new_buf = unsafe {
            realloc(
                cur as *mut c_void,
                (cap as usize) * elem,
                (new_cap as usize) * elem,
            )
        } as *mut *mut c_void;
        G_BUFFER.store(new_buf, Ordering::Relaxed);
        G_BUFFER_CAP.store(new_cap, Ordering::Relaxed);
    }
    let buf = G_BUFFER.load(Ordering::Relaxed);
    unsafe { *buf.add(len as usize) = p };
    G_BUFFER_LEN.store(len + 1, Ordering::Relaxed);
}

/// Iterate every live (non-NULL) buffered candidate. Helper for the
/// `collect` module — keeps buffer iteration semantics confined to
/// this module so the algorithm code in `collect.rs` doesn't poke
/// at `G_BUFFER` directly.
///
/// The length is snapshotted once (entries appended by the callback
/// belong to the NEXT collect), but the base pointer reloads every
/// iteration: a callback-triggered `cycle_buffer` push at the cap
/// boundary reallocs the backing array, and a cached base would read
/// freed memory (chunk 614 — pass-3 re-buffer pushes hit this).
#[inline]
pub fn for_each<F: FnMut(*mut c_void)>(mut f: F) {
    let len = G_BUFFER_LEN.load(Ordering::Relaxed);
    for i in 0..len as usize {
        let buf = G_BUFFER.load(Ordering::Relaxed);
        let p = unsafe { *buf.add(i) };
        if !p.is_null() {
            f(p);
        }
    }
}

/// Walk EVERY slot — including NULLs — exposing the raw pointer
/// and index. Needed by `cycle_collect`'s third pass which must read
/// each slot's pointer to look at its color flag and possibly clear
/// the `FLAG_BUFFERED` bit before potentially recursing into
/// `collect_white`. Same snapshot-len + reload-base discipline as
/// [`for_each`].
#[inline]
pub fn for_each_with_index<F: FnMut(usize, *mut c_void)>(mut f: F) {
    let len = G_BUFFER_LEN.load(Ordering::Relaxed);
    for i in 0..len as usize {
        let buf = G_BUFFER.load(Ordering::Relaxed);
        let p = unsafe { *buf.add(i) };
        f(i, p);
    }
}

/// Discard the processed prefix (entries `[0..processed)`) while
/// keeping candidates appended DURING the collect — pass-3 drops of
/// surviving children re-buffer through `cycle_buffer`, and the old
/// blanket length reset stranded those blocks with `FLAG_BUFFERED`
/// still set but no buffer entry (permanently un-bufferable, their
/// future cycles invisible). Called at the end of `cycle_collect`.
pub fn compact_from(processed: u32) {
    let buf = G_BUFFER.load(Ordering::Relaxed);
    let len = G_BUFFER_LEN.load(Ordering::Relaxed);
    let mut w: u32 = 0;
    for i in processed..len {
        let p = unsafe { *buf.add(i as usize) };
        if !p.is_null() {
            unsafe { *buf.add(w as usize) = p };
            w += 1;
        }
    }
    G_BUFFER_LEN.store(w, Ordering::Relaxed);
}

/// Empties slot `i`; the collect's root walk does this before it
/// frees a block that died while buffered.
#[inline]
pub(crate) fn clear_slot(i: usize) {
    let buf = G_BUFFER.load(Ordering::Relaxed);
    unsafe { *buf.add(i) = ptr::null_mut() };
}

/// Current buffer length, for the early-out fast path in
/// `cycle_collect` (skip the whole thing if 0).
#[inline]
pub fn len() -> u32 {
    G_BUFFER_LEN.load(Ordering::Relaxed)
}

/// Called from rc_dec / Obj-walk_blk's else-branch when the rc
/// stayed positive on a cyclic-shape type. Marks PURPLE + pushes
/// into the buffer (with BUFFERED gate so duplicates skip). Cheap
/// fast-path: if already buffered, return without touching anything.
///
/// Auto-triggers `cycle_collect` when the buffer length crosses
/// the auto-collect threshold.
///
/// # Safety
/// `p` is NULL or a live heap cell with a valid `HeapHeader`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_cycle_buffer(p: *mut c_void) {
    // a long-lived cell released over and over (a function value held
    // by its singleton slot and passed to every `then`) is buffered
    // after its first release: one header bit answers before the
    // walkability test reads anything else
    if !nan_box_is_cell_like(p) {
        return;
    }
    let h = p as *mut HeapHeader;
    if unsafe { (*h).flags } & FLAG_BUFFERED != 0 {
        return;
    }
    if !unsafe { has_walkable_children(p) } {
        return;
    }
    unsafe {
        set_color(h, COLOR_PURPLE);
        (*h).flags |= FLAG_BUFFERED;
    }
    buffer_push(p);
    if G_BUFFER_LEN.load(Ordering::Relaxed) >= G_THRESHOLD.load(Ordering::Relaxed) {
        unsafe { crate::collect::__torajs_cycle_collect() };
    }
}

/// [`__torajs_cycle_buffer`] for a cell nominated on behalf of a holder
/// that went away while the cell's own refcount did not move: the
/// payload of a capture box whose stake dropped but which survives.
/// An anonymous struct is never a candidate root, the same gate
/// `__torajs_obj_drop_rc` applies: its typed drop frees it without
/// the buffer scrub, so a buffered one would leave a dangling entry
/// for the next collect to read.
///
/// # Safety
/// `p` is a live heap cell with a valid `HeapHeader`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_cycle_buffer_payload(p: *mut c_void) {
    if unsafe { is_class_obj(p) }
        && unsafe { (*layout_for_class_obj(p)).flags } & CLASS_LAYOUT_FLAG_NAMED == 0
    {
        return;
    }
    unsafe { __torajs_cycle_buffer(p) }
}

/// Scrub `p` from the cycle root buffer before its memory is freed
/// via the inline class drop, the array element walk, or
/// `value_drop_heap`'s default branch. Without this, an object that
/// was added as a cycle candidate but later normal-dropped to rc=0
/// leaves a dangling pointer in the buffer; the next collect (or
/// the end-of-program exit-drain) dereferences it and segfaults.
///
/// `#[inline(never)]` mirrors the C `__attribute__((noinline))`
/// workaround: LLVM 22 -O3 LTO miscompiles the call site inside
/// `value_drop_heap` when this body is inlined cross-TU. Keep the
/// call as a real function boundary until the upstream issue is
/// narrowed.
///
/// # Safety
/// `p` is NULL or a heap cell with a valid `HeapHeader` that has not been freed yet.
#[unsafe(no_mangle)]
#[inline(never)]
pub unsafe extern "C" fn __torajs_cycle_unbuffer(p: *mut c_void) {
    if p.is_null() {
        return;
    }
    let h = p as *mut HeapHeader;
    if unsafe { (*h).flags } & FLAG_BUFFERED == 0 {
        return;
    }
    unsafe { (*h).flags &= !FLAG_BUFFERED };
    let buf = G_BUFFER.load(Ordering::Relaxed);
    let len = G_BUFFER_LEN.load(Ordering::Relaxed);
    // a block is in the buffer at most once (the FLAG_BUFFERED gate in
    // cycle_buffer), and the blocks dropped soonest are the ones pushed
    // last, so scan from the tail and stop at the hit
    for i in (0..len as usize).rev() {
        unsafe {
            if *buf.add(i) == p {
                *buf.add(i) = ptr::null_mut();
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // one process per test under nextest, so the global buffer is ours
    #[test]
    fn clear_slot_then_compact_keeps_the_rest() {
        let a = 0x1000 as *mut c_void;
        let b = 0x2000 as *mut c_void;
        let c = 0x3000 as *mut c_void;
        buffer_push(a);
        buffer_push(b);
        buffer_push(c);
        assert_eq!(len(), 3);
        clear_slot(1);
        let mut seen = Vec::new();
        for_each(|p| seen.push(p));
        assert_eq!(seen, vec![a, c]);
        compact_from(1);
        assert_eq!(len(), 1);
        let mut kept = Vec::new();
        for_each_with_index(|i, p| kept.push((i, p)));
        assert_eq!(kept, vec![(0, c)]);
    }
}
