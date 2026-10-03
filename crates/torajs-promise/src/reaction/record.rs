//! A heap reaction record whose `arg` block the runtime can see into.
//!
//! A kernel that parks its reaction as a heap record packs its state
//! into an `arg` word only its own dispatcher can read: the drop
//! kernel frees such a record without releasing the block, and the
//! cycle collector cannot walk it. Attached through
//! [`__torajs_promise_attach_then_ops`], the record carries a
//! [`ReactionOps`] saying what the block owns, and it follows the
//! inline reaction's ownership rule (`reaction.rs`, "Ownership"):
//!
//! - the block's pointer to the source is a back-pointer while the
//!   record is parked. The job gets one reference on the source when
//!   it is queued — by the source's settlement
//!   (`state::drain_callbacks`) or by the attach itself on a settled
//!   source — and the dispatcher releases it;
//! - a source dying with the record still parked hands the block to
//!   `release`, which gives back the derived promise's reference
//!   instead of dropping it: the teardown lets go of it the way it
//!   lets go of an inline derived cell, so a pending chain of records
//!   is torn down in a loop, not in one nested call per link;
//! - the cycle collector treats what `trace` reports as the source's
//!   own children, and breaks an edge by zeroing the slot address
//!   `trace` gave for it. `release` must therefore read a NULL slot
//!   as already accounted for.

use core::ffi::c_void;

use crate::layout::{MicrotaskFn, PromiseCb, STATE_PENDING, as_promise};
use torajs_rc::rc_inc_cell;

unsafe extern "C" {
    #[link_name = "__torajs_malloc"]
    fn malloc(n: usize) -> *mut c_void;
    fn __torajs_microtask_enqueue(fn_: MicrotaskFn, arg: i64);
}

/// The visit callback a [`ReactionOps::trace`] reports through — the
/// closure trace ABI: `(index, child, slot address, ctx)`.
pub type TraceVisit = unsafe extern "C" fn(i64, *mut c_void, *mut c_void, *mut c_void);

/// How the runtime sees into a record's `arg` block.
#[repr(C)]
pub struct ReactionOps {
    /// Report every non-NULL cell pointer the block holds a reference
    /// on, except the source, with the address of the word holding it.
    pub trace: unsafe extern "C" fn(arg: i64, visit: TraceVisit, ctx: *mut c_void),
    /// Release everything the block owns but the derived promise, free
    /// the block, and return that promise with its reference (NULL when
    /// the slot is empty); the source died with the reaction parked.
    pub release: unsafe extern "C" fn(arg: i64) -> *mut c_void,
}

/// `state::__torajs_promise_attach_then` for a record described by
/// `ops`; see the module doc for what the block may and may not own.
///
/// # Safety
/// `source` is a live Promise cell; `arg` is a block `ops` describes
/// and `invoke` runs.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_attach_then_ops(
    source: *mut c_void,
    invoke: MicrotaskFn,
    arg: i64,
    ops: *const ReactionOps,
) {
    let sp = as_promise(source);
    unsafe {
        (*sp).has_handler = 1;
        if (*sp).state != STATE_PENDING {
            rc_inc_cell(source);
            __torajs_microtask_enqueue(invoke, arg);
            return;
        }
        let node = malloc(core::mem::size_of::<PromiseCb>()) as *mut PromiseCb;
        (*node).invoke = invoke;
        (*node).arg = arg;
        (*node).next = (*sp).callbacks;
        (*node).ops = ops;
        (*sp).callbacks = node;
        crate::reaction::candidate::mark_list_edge(sp);
    }
}
