//! `FinalizationRegistry` (§26.2) — the registry cell, its cell
//! records and the owner-side kernels (create / register /
//! unregister / drop).
//!
//! A registry owns its cleanup callback and every cell's held value
//! strongly; each cell's target and unregister token are observed
//! through the shared registry in [`crate::registry`], the same way
//! a WeakRef observes its target. What happens when an observed
//! object dies — the cell moving to the pending list and the registry
//! joining the cleanup queue — lives in [`crate::finreg_queue`].
//!
//! ## Heap layout (72 bytes)
//!
//! ```text
//!   offset 0  : universal heap header (8B)
//!   offset 8  : cleanup      (u64 AnyValue) — [[CleanupCallback]], owned
//!   offset 16 : cells        (*mut *mut FrCell) — live cells
//!   offset 24 : cells_len    (u32)
//!   offset 28 : cells_cap    (u32)
//!   offset 32 : props        (*mut c_void) — lazy own-property bag
//!   offset 40 : pending      (*mut *mut FrCell) — cells whose target died
//!   offset 48 : pending_head (u32) — FIFO read cursor
//!   offset 52 : pending_len  (u32)
//!   offset 56 : pending_cap  (u32)
//!   offset 60 : queued       (u32) — 1 while on the cleanup queue
//!   offset 64 : queue_next   (*mut FinReg)
//! ```
//!
//! Spec [[Cells]] is `cells ∪ pending`: a cell whose target is gone
//! stays unregistrable until cleanup takes it (§26.2.3.3 walks every
//! record, empty target or not).
//!
//! ## Cell record (40 bytes)
//!
//! ```text
//!   offset 0  : fr     (*mut FinReg) — back pointer, not owned
//!   offset 8  : target (*mut c_void) — observed; NULL once it died
//!   offset 16 : held   (u64 AnyValue) — [[HeldValue]], owned
//!   offset 24 : token  (*mut c_void) — observed; NULL = empty / died
//!   offset 32 : idx    (u32) — position in `cells` or `pending`
//!   offset 36 : state  (u32) — CELL_LIVE / CELL_PENDING
//! ```
//!
//! A cell carries its index so the dying-target handler can move it
//! to `pending` in O(1): swap-remove from `cells`, fixing the index
//! of the record that took its place.

use core::ffi::c_void;
use core::ptr;

use crate::key::av_is_cell;
use crate::layout::{FLAG_STATIC_LITERAL, HeapHeader};
use crate::registry::{__torajs_weakref_registry_deregister, __torajs_weakref_registry_register};

/// `torajs_rc::Tag::FinalizationRegistry` mirror.
pub const TAG_FINREG: u16 = 31;

/// Observer kinds the shared registry dispatches to
/// [`crate::finreg_queue`]. The owner word is the cell record.
pub const OBSERVER_FR_TARGET: u32 = 3;
/// See [`OBSERVER_FR_TARGET`].
pub const OBSERVER_FR_TOKEN: u32 = 4;

/// Byte offset of [`FinReg::props`] — the bag the any-lane channels
/// and the cycle collector reach.
pub const FINREG_PROPS_OFF: usize = 32;

pub(crate) const CELL_LIVE: u32 = 0;
pub(crate) const CELL_PENDING: u32 = 1;

#[repr(C)]
pub struct FinReg {
    pub header: HeapHeader,
    pub cleanup: u64,
    pub cells: *mut *mut FrCell,
    pub cells_len: u32,
    pub cells_cap: u32,
    pub props: *mut c_void,
    pub pending: *mut *mut FrCell,
    pub pending_head: u32,
    pub pending_len: u32,
    pub pending_cap: u32,
    pub queued: u32,
    pub queue_next: *mut FinReg,
}

#[repr(C)]
pub struct FrCell {
    pub fr: *mut FinReg,
    pub target: *mut c_void,
    pub held: u64,
    pub token: *mut c_void,
    pub idx: u32,
    pub state: u32,
}

unsafe extern "C" {
    #[link_name = "__torajs_libc_malloc"]
    fn malloc(n: usize) -> *mut c_void;
    #[link_name = "__torajs_libc_free"]
    fn free(p: *mut c_void);
    #[link_name = "__torajs_realloc"]
    fn realloc(p: *mut c_void, old_size: usize, new_size: usize) -> *mut c_void;
    #[link_name = "__torajs_free"]
    fn free_sized(p: *mut c_void, size: usize);
    fn __torajs_rc_inc(p: *mut c_void);
    fn __torajs_value_drop_heap(p: *mut c_void);
    /// torajs-cycle — a registry that lost one of several references
    /// is a possible cycle root through its callback / held values.
    fn __torajs_cycle_buffer(p: *mut c_void);
    fn __torajs_cycle_unbuffer(p: *mut c_void);
    fn __torajs_subclass_drop_entry(p: *mut c_void);
}

const SLOT: usize = core::mem::size_of::<*mut FrCell>();

/// Append `cell` to a (ptr, len, cap) vector, recording its index.
///
/// # Safety
/// The three pointers address one vector of this registry.
pub(crate) unsafe fn vec_push(
    buf: *mut *mut *mut FrCell,
    len: *mut u32,
    cap: *mut u32,
    cell: *mut FrCell,
) {
    unsafe {
        if *len == *cap {
            let new_cap = if *cap == 0 { 4 } else { *cap * 2 };
            *buf = realloc(
                *buf as *mut c_void,
                *cap as usize * SLOT,
                new_cap as usize * SLOT,
            ) as *mut *mut FrCell;
            *cap = new_cap;
        }
        *(*buf).add(*len as usize) = cell;
        (*cell).idx = *len;
        *len += 1;
    }
}

/// Swap-remove the live cell at `idx`, fixing the moved record's
/// index.
///
/// # Safety
/// `idx < cells_len` and `fr` is a live registry.
pub(crate) unsafe fn cells_swap_remove(fr: *mut FinReg, idx: u32) {
    unsafe {
        let last = (*fr).cells_len - 1;
        if idx != last {
            let moved = *(*fr).cells.add(last as usize);
            *(*fr).cells.add(idx as usize) = moved;
            (*moved).idx = idx;
        }
        (*fr).cells_len = last;
    }
}

/// Stop observing the cell's target and token. Must run before the
/// record is freed while either is still set.
///
/// # Safety
/// `cell` is a live record.
pub(crate) unsafe fn cell_detach(cell: *mut FrCell) {
    unsafe {
        if !(*cell).target.is_null() {
            __torajs_weakref_registry_deregister((*cell).target, OBSERVER_FR_TARGET, cell.cast());
            (*cell).target = ptr::null_mut();
        }
        if !(*cell).token.is_null() {
            __torajs_weakref_registry_deregister((*cell).token, OBSERVER_FR_TOKEN, cell.cast());
            (*cell).token = ptr::null_mut();
        }
    }
}

/// Release an owned AnyValue word.
#[inline]
pub(crate) unsafe fn av_release(v: u64) {
    if av_is_cell(v) {
        unsafe { __torajs_value_drop_heap(v as *mut c_void) };
    }
}

/// `new FinalizationRegistry(cleanup)` kernel — a fresh `+1` registry
/// retaining `cleanup` (borrowed). The callable check is the
/// constructor's (§26.2.1.1 step 2), not this kernel's.
///
/// # Safety
/// `cleanup` is a valid AnyValue.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_finreg_create(cleanup: u64) -> *mut c_void {
    let fr = unsafe { malloc(core::mem::size_of::<FinReg>()) } as *mut FinReg;
    if av_is_cell(cleanup) {
        unsafe { __torajs_rc_inc(cleanup as *mut c_void) };
    }
    unsafe {
        fr.write(FinReg {
            header: HeapHeader {
                refcount: 1,
                type_tag: TAG_FINREG,
                flags: 0,
            },
            cleanup,
            cells: ptr::null_mut(),
            cells_len: 0,
            cells_cap: 0,
            props: ptr::null_mut(),
            pending: ptr::null_mut(),
            pending_head: 0,
            pending_len: 0,
            pending_cap: 0,
            queued: 0,
            queue_next: ptr::null_mut(),
        });
    }
    fr.cast()
}

/// §26.2.3.2 steps 5–8 — record a cell. The caller has already
/// validated `target` / `token` (CanBeHeldWeakly) and SameValue
/// (target, held); `token` is NULL for an empty token. `held` is
/// borrowed and retained.
///
/// # Safety
/// `fr` is a live registry; `target` a live heap cell; `token` NULL
/// or a live heap cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_finreg_register(
    fr: *mut c_void,
    target: *mut c_void,
    held: u64,
    token: *mut c_void,
) {
    let fr = fr as *mut FinReg;
    if av_is_cell(held) {
        unsafe { __torajs_rc_inc(held as *mut c_void) };
    }
    let cell = unsafe { malloc(core::mem::size_of::<FrCell>()) } as *mut FrCell;
    unsafe {
        cell.write(FrCell {
            fr,
            target,
            held,
            token,
            idx: 0,
            state: CELL_LIVE,
        });
        vec_push(
            &raw mut (*fr).cells,
            &raw mut (*fr).cells_len,
            &raw mut (*fr).cells_cap,
            cell,
        );
        __torajs_weakref_registry_register(target, OBSERVER_FR_TARGET, cell.cast());
        if !token.is_null() {
            __torajs_weakref_registry_register(token, OBSERVER_FR_TOKEN, cell.cast());
        }
    }
}

/// §26.2.3.3 steps 4–5 — remove every cell (live or pending) whose
/// token is `token`; 1 when at least one went. Held values are
/// released only after both vectors are consistent again: a release
/// can run arbitrary drops, and a drop can reach this registry's
/// observers.
///
/// # Safety
/// `fr` is a live registry; `token` a legal weak key.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_finreg_unregister(fr: *mut c_void, token: *mut c_void) -> i32 {
    let fr = fr as *mut FinReg;
    let mut released: *mut u64 = ptr::null_mut();
    let mut n_released = 0usize;
    unsafe {
        let mut i = 0;
        while i < (*fr).cells_len {
            let cell = *(*fr).cells.add(i as usize);
            if (*cell).token == token {
                cells_swap_remove(fr, i);
                take_cell(cell, &mut released, &mut n_released);
            } else {
                i += 1;
            }
        }
        let head = (*fr).pending_head;
        let mut w = head;
        for r in head..(*fr).pending_len {
            let cell = *(*fr).pending.add(r as usize);
            if (*cell).token == token {
                take_cell(cell, &mut released, &mut n_released);
            } else {
                *(*fr).pending.add(w as usize) = cell;
                (*cell).idx = w;
                w += 1;
            }
        }
        (*fr).pending_len = w;
        for k in 0..n_released {
            av_release(*released.add(k));
        }
        if !released.is_null() {
            free_sized(released.cast(), n_released.next_power_of_two() * 8);
        }
    }
    (n_released != 0) as i32
}

/// Detach and free `cell`, appending its held value to `out`
/// (grown by powers of two, sized by count).
unsafe fn take_cell(cell: *mut FrCell, out: &mut *mut u64, n: &mut usize) {
    unsafe {
        cell_detach(cell);
        if n.is_power_of_two() || *n == 0 {
            let old = if *n == 0 { 0 } else { *n * 8 };
            let new = (*n + 1).next_power_of_two() * 8;
            *out = realloc(out.cast(), old, new) as *mut u64;
        }
        *out.add(*n) = (*cell).held;
        *n += 1;
        free(cell.cast());
    }
}

/// Universal-drop entry (`Tag::FinalizationRegistry`). On the last
/// reference every observer is detached first — held values and the
/// callback are released only after no cell of this registry can be
/// reached from the shared registry, since releasing them may kill a
/// target this registry still watches.
///
/// # Safety
/// `p` is NULL or a live registry.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_finreg_drop(p: *mut c_void) {
    if p.is_null() {
        return;
    }
    let fr = p as *mut FinReg;
    unsafe {
        if (*fr).header.flags & FLAG_STATIC_LITERAL != 0 {
            return;
        }
        (*fr).header.refcount -= 1;
        if (*fr).header.refcount != 0 {
            __torajs_cycle_buffer(p);
            return;
        }
        // scrubbed before the held values go, so a chain released
        // newest first meets each cell at the buffer's tail
        __torajs_cycle_unbuffer(p);
        let head = (*fr).pending_head as usize;
        let (nc, np) = ((*fr).cells_len as usize, (*fr).pending_len as usize);
        for i in 0..nc {
            cell_detach(*(*fr).cells.add(i));
        }
        for i in head..np {
            cell_detach(*(*fr).pending.add(i));
        }
        for i in 0..nc {
            let cell = *(*fr).cells.add(i);
            av_release((*cell).held);
            free(cell.cast());
        }
        for i in head..np {
            let cell = *(*fr).pending.add(i);
            av_release((*cell).held);
            free(cell.cast());
        }
        free_sized((*fr).cells.cast(), (*fr).cells_cap as usize * SLOT);
        free_sized((*fr).pending.cast(), (*fr).pending_cap as usize * SLOT);
        av_release((*fr).cleanup);
        if !(*fr).props.is_null() {
            __torajs_value_drop_heap((*fr).props);
        }
        if (*fr).header.flags & crate::layout::FLAG_SUBCLASSED != 0 {
            __torajs_subclass_drop_entry(p);
        }
        free(p);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layouts() {
        assert_eq!(core::mem::size_of::<FinReg>(), 72);
        assert_eq!(core::mem::offset_of!(FinReg, cleanup), 8);
        assert_eq!(core::mem::offset_of!(FinReg, props), FINREG_PROPS_OFF);
        assert_eq!(core::mem::offset_of!(FinReg, pending), 40);
        assert_eq!(core::mem::offset_of!(FinReg, queue_next), 64);
        assert_eq!(core::mem::size_of::<FrCell>(), 40);
        assert_eq!(core::mem::offset_of!(FrCell, held), 16);
        assert_eq!(core::mem::offset_of!(FrCell, idx), 32);
    }
}
