//! Deferred Free step for the trial-deletion collector.
//!
//! Bacon & Rajan's CollectWhite conceptually separates *collecting*
//! the white set from *freeing* it. The pre-fix port freed each
//! WHITE block inline while the collect pass was still iterating the
//! root buffer — but a WHITE block shared by TWO white parents is
//! freed via the first parent's recursion while the second parent's
//! slot still points at it, and the second parent's survivor sweep
//! then reads the freed header. mmalloc's free-list next pointer
//! overwrites the refcount word, so the corpse fakes `rc > 0`, gets
//! re-dropped AND re-buffered — the next collect round walks the
//! stale root straight into reused memory (SIGSEGV; the rotation-165
//! error-proto-cluster probe caught it at exit drain).
//!
//! Fix: `collect_white` only sweeps + pushes the corpse here; after
//! the collect pass finishes, [`finalize_all`] runs
//!
//! - **Pass A** while every corpse is still allocated (so a fellow
//!   corpse's `rc == 0` reads truthfully): closure slot pre-clear,
//!   dynobj key release, StringWrapper [[StringData]] release.
//! - **Pass B**: the actual frees (closure drop_fn / sized dynobj
//!   free / arr spill + generic free).
//!
//! Same `AtomicPtr` static shape as `buffer.rs` (single-threaded
//! today, multi-thread-ready form).

use core::ffi::c_void;
use core::ptr;
use std::sync::atomic::{AtomicPtr, AtomicU32, Ordering};

use crate::arr::arr_spilled_data;
use crate::collect::{clear_child_slot, for_each_child};
use crate::dynobj::{
    DYNOBJ_HEADER_BYTES, dynobj_entries_len, dynobj_key_at, dynobj_store_bytes, dynobj_store_ptr,
};
use crate::layout::{
    CLOSURE_DROP_FN_OFF, HeapHeader, STRING_WRAPPER_CELL_OFF, TAG_BOOLEAN_WRAPPER, TAG_CLOSURE,
    TAG_DYNOBJ, TAG_NUMBER_WRAPPER, TAG_STRING_WRAPPER, is_class_obj,
};

unsafe extern "C" {
    /// See `collect.rs` — the same mmalloc trio.
    #[link_name = "__torajs_libc_free"]
    fn free(p: *mut c_void);
    #[link_name = "__torajs_free"]
    fn free_sized(p: *mut c_void, size: usize);
    #[link_name = "__torajs_realloc"]
    fn realloc(p: *mut c_void, old_size: usize, new_size: usize) -> *mut c_void;
    fn __torajs_str_drop(s: *mut c_void);
    fn __torajs_value_drop_heap(p: *mut c_void);
}

static D_LIST: AtomicPtr<*mut c_void> = AtomicPtr::new(ptr::null_mut());
static D_LEN: AtomicU32 = AtomicU32::new(0);
static D_CAP: AtomicU32 = AtomicU32::new(0);

/// Append a swept WHITE corpse for the post-pass free. Same
/// grow-by-doubling shape as `buffer::buffer_push`.
pub(crate) fn defer_push(p: *mut c_void) {
    let len = D_LEN.load(Ordering::Relaxed);
    let cap = D_CAP.load(Ordering::Relaxed);
    if len == cap {
        let new_cap = if cap == 0 { 64 } else { cap * 2 };
        let cur = D_LIST.load(Ordering::Relaxed);
        let elem = core::mem::size_of::<*mut c_void>();
        let new_buf = unsafe {
            realloc(
                cur as *mut c_void,
                (cap as usize) * elem,
                (new_cap as usize) * elem,
            )
        } as *mut *mut c_void;
        D_LIST.store(new_buf, Ordering::Relaxed);
        D_CAP.store(new_cap, Ordering::Relaxed);
    }
    let buf = D_LIST.load(Ordering::Relaxed);
    unsafe { *buf.add(len as usize) = p };
    D_LEN.store(len + 1, Ordering::Relaxed);
}

/// Run the deferred Free step over everything `collect_white` swept
/// this round, then reset the list.
///
/// # Safety
/// Every listed block is a swept WHITE corpse: BLACK-recolored,
/// unbuffered, rc == 0, child slots either cleared or pointing at
/// survivors / fellow corpses. Must run before `compact_from` so
/// re-buffer pushes from closure drop_fns land in the kept region.
pub(crate) unsafe fn finalize_all() {
    let len = D_LEN.load(Ordering::Relaxed);
    if len == 0 {
        return;
    }
    let buf = D_LIST.load(Ordering::Relaxed);
    // Pass A — pre-free work while EVERY corpse is still allocated.
    for i in 0..len as usize {
        let p = unsafe { *buf.add(i) };
        let tag = unsafe { (*(p as *const HeapHeader)).type_tag };
        if tag == TAG_CLOSURE {
            // Zero every slot the cycle machinery already accounted:
            // fellow corpses (rc == 0 — drop_fn has no rc gate and
            // would re-drop a block this drain is about to free) and
            // walkable survivors (mark trial-decremented this dying
            // parent's edge and scan_black never restores a WHITE
            // parent's edges, so that unrestored trial-dec IS the
            // release — drop_fn releasing the slot too would charge
            // the edge twice; collect_white's second-sweep rule in
            // closure form). Non-walkable captures (Str / boxed
            // scalars …) were never trial-decremented and stay for
            // drop_fn. Truthful only in pass A: a freed sibling's
            // header word gets overwritten by the allocator (the
            // exact bug this module exists for).
            unsafe {
                for_each_child(p, |i2, child| {
                    if (*(child as *const HeapHeader)).refcount == 0
                        || crate::layout::has_walkable_children(child)
                    {
                        clear_child_slot(p, i2);
                    }
                });
            }
        } else if tag == TAG_DYNOBJ {
            // Dict key walk — every live entry still owns its key Str
            // (the sweeps only touched value slots).
            let n = unsafe { dynobj_entries_len(p) };
            for k in 0..n {
                let key = unsafe { dynobj_key_at(p, k) };
                if !key.is_null() {
                    unsafe { __torajs_str_drop(key) };
                }
            }
        } else if tag == TAG_STRING_WRAPPER {
            // The [[StringData]] Str cell is a leaf the walk never
            // touched — release it like __torajs_string_wrapper_drop.
            let cell =
                unsafe { *((p as *const u8).add(STRING_WRAPPER_CELL_OFF) as *const *mut c_void) };
            if !cell.is_null() {
                unsafe { __torajs_value_drop_heap(cell) };
            }
        }
    }
    // Pass B — the frees.
    for i in 0..len as usize {
        let p = unsafe { *buf.add(i) };
        let tag = unsafe { (*(p as *const HeapHeader)).type_tag };
        if tag == TAG_CLOSURE {
            // drop_fn releases surviving capture slots (cleared cycle
            // edges no-op through its NULL/NaN gates) and frees the
            // env block itself.
            let drop_raw = unsafe { *((p as *const u8).add(CLOSURE_DROP_FN_OFF) as *const u64) };
            if drop_raw != 0 {
                let drop_fn = unsafe {
                    core::mem::transmute::<u64, unsafe extern "C" fn(*mut c_void)>(drop_raw)
                };
                unsafe { drop_fn(p) };
            }
        } else if tag == TAG_DYNOBJ {
            // Two sized frees since the store split — the independent
            // index+entries block first, then the header cell.
            unsafe {
                free_sized(dynobj_store_ptr(p), dynobj_store_bytes(p));
                free_sized(p, DYNOBJ_HEADER_BYTES);
            }
        } else {
            if !matches!(
                tag,
                TAG_NUMBER_WRAPPER | TAG_STRING_WRAPPER | TAG_BOOLEAN_WRAPPER
            ) && !unsafe { is_class_obj(p) }
            {
                // TAG_ARR — a grown array spilled its slots to an
                // independent buffer; release it alongside the cell.
                let spill = unsafe { arr_spilled_data(p) };
                if !spill.is_null() {
                    unsafe { free(spill as *mut c_void) };
                }
            }
            unsafe { free(p) };
        }
    }
    D_LEN.store(0, Ordering::Relaxed);
}
