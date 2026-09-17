//! `FinalizationRegistry` death handling — what the shared observer
//! registry calls when a cell's target or token dies, and the
//! process-global cleanup queue the program drains.
//!
//! The handlers run in the middle of a `dec_ref` hit-zero path
//! (`torajs_rc::weak_gate`), so they **never run user code** and
//! never release anything: a dying target only moves its cell to the
//! registry's pending list and puts the registry on the queue, which
//! holds one reference to it until the drain has run every pending
//! callback (§9.13 HostEnqueueFinalizationRegistryCleanupJob — the
//! job keeps the registry alive).
//!
//! Two self-references need care:
//! - `token === target`: both observer nodes sit on the one
//!   TargetCell the walk is freeing, so each handler only clears its
//!   own field and touches no registry node.
//! - `target === fr`: the registry itself is dying. Enqueueing would
//!   resurrect a block about to be freed, so the handler only clears
//!   the field and the registry's drop frees the cell.

use core::ffi::c_void;
use core::ptr;
use std::sync::atomic::{AtomicPtr, Ordering};

use crate::finreg::{
    CELL_LIVE, CELL_PENDING, FinReg, FrCell, cell_detach, cells_swap_remove, vec_push,
};

/// Cleanup queue — FIFO of registries with pending cells, each holding
/// the queue's reference. Single mutator today; `AtomicPtr` keeps the
/// §6.2 multi-thread-ready shape.
static QUEUE_HEAD: AtomicPtr<FinReg> = AtomicPtr::new(ptr::null_mut());
static QUEUE_TAIL: AtomicPtr<FinReg> = AtomicPtr::new(ptr::null_mut());

unsafe extern "C" {
    #[link_name = "__torajs_libc_free"]
    fn free(p: *mut c_void);
}

/// `OBSERVER_FR_TARGET` — the cell's target died.
///
/// # Safety
/// `cell` is a live live-state record whose target is `target`.
pub(crate) unsafe fn target_died(cell: *mut FrCell, target: *mut c_void) {
    unsafe {
        (*cell).target = ptr::null_mut();
        let fr = (*cell).fr;
        if fr.cast() == target || (*cell).state != CELL_LIVE {
            return;
        }
        cells_swap_remove(fr, (*cell).idx);
        (*cell).state = CELL_PENDING;
        vec_push(
            &raw mut (*fr).pending,
            &raw mut (*fr).pending_len,
            &raw mut (*fr).pending_cap,
            cell,
        );
        if (*fr).queued == 0 {
            (*fr).queued = 1;
            // The queue's reference. Registries are never static
            // literals, so the header word is the whole story.
            (*fr).header.refcount += 1;
            (*fr).queue_next = ptr::null_mut();
            let tail = QUEUE_TAIL.load(Ordering::Relaxed);
            if tail.is_null() {
                QUEUE_HEAD.store(fr, Ordering::Relaxed);
            } else {
                (*tail).queue_next = fr;
            }
            QUEUE_TAIL.store(fr, Ordering::Relaxed);
        }
    }
}

/// `OBSERVER_FR_TOKEN` — the cell's unregister token died; nothing
/// can name the cell any more, so only the field goes.
///
/// # Safety
/// `cell` is a live record.
pub(crate) unsafe fn token_died(cell: *mut FrCell) {
    unsafe { (*cell).token = ptr::null_mut() };
}

/// Take the registry at the head of the cleanup queue, transferring
/// the queue's reference to the caller (who releases it after the
/// drain). NULL when the queue is empty.
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_finreg_queue_pop() -> *mut c_void {
    let fr = QUEUE_HEAD.load(Ordering::Relaxed);
    if fr.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        let next = (*fr).queue_next;
        QUEUE_HEAD.store(next, Ordering::Relaxed);
        if next.is_null() {
            QUEUE_TAIL.store(ptr::null_mut(), Ordering::Relaxed);
        }
        (*fr).queue_next = ptr::null_mut();
        (*fr).queued = 0;
    }
    fr.cast()
}

/// §26.2.3.x CleanupFinalizationRegistry step 3 — remove the oldest
/// pending cell and hand its held value to the caller (ownership
/// transfers). 0 when nothing is pending.
///
/// # Safety
/// `fr` is a live registry; `out` is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_finreg_take_pending(fr: *mut c_void, out: *mut u64) -> i32 {
    let fr = fr as *mut FinReg;
    unsafe {
        if (*fr).pending_head == (*fr).pending_len {
            (*fr).pending_head = 0;
            (*fr).pending_len = 0;
            return 0;
        }
        let cell = *(*fr).pending.add((*fr).pending_head as usize);
        (*fr).pending_head += 1;
        cell_detach(cell);
        *out = (*cell).held;
        free(cell.cast());
    }
    1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::finreg::{
        __torajs_finreg_create, __torajs_finreg_drop, __torajs_finreg_register,
        __torajs_finreg_unregister,
    };
    use crate::layout::HeapHeader;
    use crate::registry::__torajs_weakref_target_dying;

    // NaN-box immediates — never rc'd, so the panicking test stubs
    // for rc_inc / value_drop_heap stay untouched.
    const NUM_1: u64 = 0xFFFE_0000_0000_0001;
    const NUM_2: u64 = 0xFFFE_0000_0000_0002;

    fn fake_cell() -> *mut c_void {
        Box::into_raw(Box::new(HeapHeader {
            refcount: 1,
            type_tag: 1,
            flags: 0,
        }))
        .cast()
    }

    fn fr_of(p: *mut c_void) -> *mut FinReg {
        p.cast()
    }

    #[test]
    fn target_death_queues_then_drains_in_order() {
        unsafe {
            let fr = __torajs_finreg_create(NUM_1);
            let (t1, t2, tok) = (fake_cell(), fake_cell(), fake_cell());
            __torajs_finreg_register(fr, t1, NUM_1, ptr::null_mut());
            __torajs_finreg_register(fr, t2, NUM_2, tok);
            __torajs_weakref_target_dying(t2);
            __torajs_weakref_target_dying(t1);
            assert_eq!((*fr_of(fr)).cells_len, 0);
            assert_eq!((*fr_of(fr)).header.refcount, 2);
            assert_eq!(__torajs_finreg_queue_pop(), fr);
            assert!(__torajs_finreg_queue_pop().is_null());
            let mut v = 0u64;
            assert_eq!(__torajs_finreg_take_pending(fr, &mut v), 1);
            assert_eq!(v, NUM_2);
            assert_eq!(__torajs_finreg_take_pending(fr, &mut v), 1);
            assert_eq!(v, NUM_1);
            assert_eq!(__torajs_finreg_take_pending(fr, &mut v), 0);
            // The token observer went with its cell: its death is a no-op.
            __torajs_weakref_target_dying(tok);
            __torajs_finreg_drop(fr);
            __torajs_finreg_drop(fr);
        }
    }

    #[test]
    fn unregister_reaches_pending_cells_and_token_equals_target() {
        unsafe {
            let fr = __torajs_finreg_create(NUM_1);
            let (t1, t2) = (fake_cell(), fake_cell());
            __torajs_finreg_register(fr, t1, NUM_1, t1);
            __torajs_finreg_register(fr, t2, NUM_2, t1);
            __torajs_weakref_target_dying(t2);
            assert_eq!(__torajs_finreg_unregister(fr, t1), 1);
            assert_eq!(__torajs_finreg_unregister(fr, t1), 0);
            assert_eq!((*fr_of(fr)).cells_len, 0);
            let mut v = 0u64;
            assert_eq!(__torajs_finreg_take_pending(fr, &mut v), 0);
            let q = __torajs_finreg_queue_pop();
            assert_eq!(q, fr);
            __torajs_finreg_drop(fr);
            __torajs_finreg_drop(fr);
        }
    }

    #[test]
    fn registry_as_its_own_target_does_not_requeue() {
        unsafe {
            let fr = __torajs_finreg_create(NUM_1);
            __torajs_finreg_register(fr, fr, NUM_2, fr);
            __torajs_weakref_target_dying(fr);
            assert_eq!((*fr_of(fr)).queued, 0);
            assert_eq!((*fr_of(fr)).header.refcount, 1);
            __torajs_finreg_drop(fr);
        }
    }
}
