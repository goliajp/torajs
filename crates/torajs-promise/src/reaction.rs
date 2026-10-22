//! Inline reaction — the one-allocation form of `.then` / `.catch`.
//!
//! The derived promise a typed then / catch kernel answers is PENDING
//! until the reaction runs, so the cell itself carries the reaction
//! (`Promise::rx_*`, layout in `layout.rs`): the source it waits on,
//! the handler, the two repr codes and its link in the source's
//! callback list. JSC keeps a single `JSPromise` reaction the same
//! way. Nothing else is allocated per `then` — a chain of a million
//! links is a million 64-byte cells, not three million blocks.
//!
//! ## Ownership
//!
//! - `rx_source` names the source. While the reaction waits in the
//!   source's list it is a back-pointer, not a reference: the list
//!   entry already holds the derived cell, and a reference back would
//!   make every pending `then` a two-cell ring only the cycle
//!   collector could reclaim. When the reaction is queued — by the
//!   source's settlement (`state::drain_callbacks`) or at once, here,
//!   on a settled source — it becomes one reference, released after
//!   the reaction ran (the dispatcher reads `source.value` across the
//!   microtask delay). A source dying with the reaction still parked
//!   clears it.
//! - The source's list entry holds one reference on the derived cell
//!   (RFC 20260720-promise-any-cb): a discarding call site drops the
//!   returned promise before the job fires, and the dispatcher would
//!   otherwise settle a recycled cell. Released by the dispatcher
//!   after settling — or, when settling queued the derived cell's own
//!   sole reaction, kept as that job's reference on its source (for a
//!   sole parked async frame, as the frame's `awaiting`).
//! - `rx_cb` with [`RX_CLOSURE`] holds one reference on the env,
//!   released after the call.
//!
//! A derived cell dying with its reaction unfired (its source died
//! first, or the cycle collector tore down a ring through it)
//! releases the env in `pool::__torajs_promise_drop`; a queued one
//! cannot die, the queue holds it.

use core::ffi::c_void;
use core::hint::select_unpredictable;
use core::mem::transmute;
use core::ptr;

use crate::frame::AsyncFrame;
use crate::layout::{
    CB_FRAME_TAG, CB_INLINE_TAG, Promise, REPR_ANY, REPR_HEAP, REPR_STR, REPR_VOID, RX_CATCH,
    RX_CLOSURE, RX_INTEREST_MASK, RX_NONE, RX_PARAM_ANY, RX_THEN, STATE_FULFILLED, STATE_PENDING,
    STATE_REJECTED, as_promise, inline_link,
};
use crate::pool::{__torajs_promise_drop, release_cell};
use crate::state::{__torajs_promise_reject, __torajs_promise_resolve};
use crate::then::{ThenCbI64, call_handler, stamp_result_repr};
use crate::then_box::{handler_value, reject_on_pending_throw, settle_handler_return};
use queue::queue_idle;
use torajs_rc::rc_inc_cell;

pub mod candidate;
mod frame;
mod queue;
pub mod record;

unsafe extern "C" {
    fn __torajs_value_drop_heap(p: *mut c_void);
    fn __torajs_rc_inc(p: *mut c_void);
    fn __torajs_microtask_enqueue(fn_: crate::layout::MicrotaskFn, arg: i64);
    fn __torajs_microtask_enqueue_cell(
        fn_: crate::layout::MicrotaskFn,
        cell: *mut c_void,
    ) -> *mut c_void;
}

/// Park the reaction `(kind, cb)` in `derived`, take its stakes and
/// hook `derived` into `source`'s list — or enqueue it at once when
/// `source` has already settled. `derived` must be a fresh
/// pending cell with no reaction; `cb` is a `ThenCbI64` fn pointer,
/// or a live closure env when `kind` carries [`RX_CLOSURE`].
#[inline(always)]
pub(crate) unsafe fn attach_inline(
    source: *mut c_void,
    derived: *mut c_void,
    kind: u8,
    cb: *mut c_void,
    ret_repr: u8,
    param_repr: u8,
) {
    let sp = as_promise(source);
    let dp = as_promise(derived);
    unsafe {
        (*dp).rx_kind = kind;
        (*dp).rx_ret_repr = ret_repr;
        (*dp).rx_param_repr = param_repr;
        (*dp).rx_source = source;
        (*dp).rx_cb = cb;
        rc_inc_cell(derived);
        if kind & RX_CLOSURE != 0 {
            rc_inc_cell(cb);
        }
        // §27.2.5.4.1 PerformPromiseThen step 12 — the source is handled
        // from here on (the derived promise is the live complaint).
        (*sp).has_handler = 1;
        if (*sp).state != STATE_PENDING {
            rc_inc_cell(source);
            __torajs_microtask_enqueue(inline_reaction_dispatch, derived as i64);
            return;
        }
        (*dp).rx_next = (*sp).callbacks;
        (*sp).callbacks = inline_link(dp);
        if kind & RX_CLOSURE != 0 {
            (*dp).header.flags |= candidate::FLAG_RX_ENV;
            candidate::mark_list_edge(sp);
        }
    }
}

/// [`attach_inline`] of a plain handler (no [`RX_CLOSURE`]) to a source
/// known to be pending, into a cell fresh from the allocator: nothing
/// is queued and no env is held, so it makes no call.
#[inline(always)]
pub(crate) unsafe fn attach_plain_pending(
    sp: *mut Promise,
    dp: *mut Promise,
    kind: u8,
    cb: *mut c_void,
    ret_repr: u8,
    param_repr: u8,
) {
    unsafe {
        (*dp).rx_kind = kind;
        (*dp).rx_ret_repr = ret_repr;
        (*dp).rx_param_repr = param_repr;
        (*dp).rx_source = sp as *mut c_void;
        (*dp).rx_cb = cb;
        // the list's stake
        rc_inc_cell(dp as *mut c_void);
        (*sp).has_handler = 1;
        (*dp).rx_next = (*sp).callbacks;
        (*sp).callbacks = inline_link(dp);
    }
}

/// [`attach_inline`] of a plain handler to a source that has already
/// settled, into a cell fresh from the allocator: the reaction is
/// queued at once and `dp` is answered out of the queue's push, so an
/// entry point that returns it keeps no frame.
#[inline(always)]
pub(crate) unsafe fn attach_plain_settled(
    sp: *mut Promise,
    dp: *mut Promise,
    kind: u8,
    cb: *mut c_void,
    ret_repr: u8,
    param_repr: u8,
) -> *mut c_void {
    unsafe {
        (*dp).rx_kind = kind;
        (*dp).rx_ret_repr = ret_repr;
        (*dp).rx_param_repr = param_repr;
        (*dp).rx_source = sp as *mut c_void;
        (*dp).rx_cb = cb;
        // the job's stakes on both cells
        rc_inc_cell(dp as *mut c_void);
        (*sp).has_handler = 1;
        rc_inc_cell(sp as *mut c_void);
        __torajs_microtask_enqueue_cell(inline_reaction_dispatch, dp as *mut c_void)
    }
}

/// Queue the reaction parked in `derived`, now out of the list of
/// `source`, which just settled: the job takes its stake on `source`.
#[inline(always)]
pub(crate) unsafe fn queue_inline(source: *mut Promise, derived: *mut Promise) {
    unsafe {
        (*derived).rx_next = ptr::null_mut();
        rc_inc_cell(source as *mut c_void);
        __torajs_microtask_enqueue(inline_reaction_dispatch, derived as i64);
    }
}

/// PromiseReactionJob for an inline reaction: `arg` is the derived
/// cell. A link of a `then` chain whose reaction would be the only
/// job pending runs here as well, one link after another, instead of
/// going through the queue ([`settle_into_chain`]).
pub(crate) unsafe extern "C" fn inline_reaction_dispatch(arg: i64) {
    let dp = arg as *mut Promise;
    match unsafe { run_inline_reaction(dp) } {
        Stake::Kept => unsafe { __torajs_promise_drop(dp as *mut c_void) },
        Stake::Queued => {}
        Stake::RunNext(next) => unsafe { run_chain(next) },
        Stake::RunFrame(f) => unsafe { frame::resume(f) },
    }
}

/// The rest of a chain that runs in one job: a loop of its own, so
/// the job of a lone reaction keeps the shape of a single run.
#[inline(never)]
unsafe fn run_chain(mut dp: *mut Promise) {
    loop {
        match unsafe { run_inline_reaction(dp) } {
            Stake::Kept => return unsafe { __torajs_promise_drop(dp as *mut c_void) },
            Stake::Queued => return,
            Stake::RunNext(next) => dp = next,
            Stake::RunFrame(f) => return unsafe { frame::resume(f) },
        }
    }
}

// the dispatcher picks the interest a settlement fires by adding the
// rejected bit to RX_THEN
const _: () = assert!(RX_CATCH == RX_THEN + 1);

/// What settling a derived cell did with the dispatcher's stake on it.
enum Stake {
    /// Still the dispatcher's, to release.
    Kept,
    /// Handed to the queued job of the cell's sole reaction.
    Queued,
    /// Handed to the cell's sole reaction, which runs next, here.
    RunNext(*mut Promise),
    /// Handed to the async frame that was the cell's sole waiter, as
    /// its `awaiting`; the frame resumes next, here.
    RunFrame(*mut AsyncFrame),
}

/// Run the reaction parked in `dp` and answer what became of the
/// dispatcher's stake on `dp`. The reaction words are taken out of
/// the cell before the handler runs — a handler may attach to the
/// derived promise or release the last outside reference to either
/// cell, and the cell must already read as reaction-free when the
/// stakes come off.
#[inline(always)]
unsafe fn run_inline_reaction(dp: *mut Promise) -> Stake {
    unsafe {
        let source = (*dp).rx_source;
        let cb = (*dp).rx_cb;
        let kind = (*dp).rx_kind;
        let ret_repr = (*dp).rx_ret_repr;
        let param_repr = (*dp).rx_param_repr;
        (*dp).rx_kind = RX_NONE;
        (*dp).rx_source = ptr::null_mut();
        (*dp).rx_cb = ptr::null_mut();
        (*dp).rx_next = ptr::null_mut();
        let sp = as_promise(source);
        let derived = dp as *mut c_void;
        let rejected = (*sp).state == STATE_REJECTED;
        // `.then` fires on a fulfillment, `.catch` on a rejection
        let fires = kind & RX_INTEREST_MASK == RX_THEN + rejected as u8;
        let stake = if fires {
            let (param_any, closure) = (kind & RX_PARAM_ANY != 0, kind & RX_CLOSURE != 0);
            match handler_value(sp, param_any, param_repr, cb, closure) {
                Ok(v) => {
                    let result = if closure {
                        call_handler(cb, v)
                    } else {
                        transmute::<*mut c_void, ThenCbI64>(cb)(v)
                    };
                    // a Void-ret handler leaves garbage in its return register
                    let result = select_unpredictable(ret_repr == REPR_VOID, 0, result);
                    settle_into_chain(dp, ret_repr, result)
                }
                Err(twin_answer) => settle_into_chain(dp, REPR_ANY, twin_answer),
            }
        } else {
            // The handler's interest does not match: forward the
            // settlement unchanged so a downstream handler sees it.
            // The value keeps its own form and a stake of its own (the
            // source may be released first).
            stamp_result_repr(derived, (*sp).value_repr);
            (*dp).value_is_heap = (*sp).value_is_heap;
            if (*sp).value_is_heap != 0 && (*sp).value != 0 {
                __torajs_rc_inc((*sp).value as *mut c_void);
            }
            if rejected {
                __torajs_promise_reject(derived, (*sp).value);
            } else {
                __torajs_promise_resolve(derived, (*sp).value);
            }
            Stake::Kept
        };
        // releasing the live source cell or an env runs no job and
        // queues none, so a reaction left to run next is still next
        release_cell(source);
        if kind & RX_CLOSURE != 0 {
            __torajs_value_drop_heap(cb);
        }
        stake
    }
}

/// Settle `dp` with its handler's return `value`. When the return is
/// a form that can be neither adopted nor a thenable and `dp`'s list
/// is one inline reaction — a link of a `then` chain — this fulfills
/// `dp` and hands the dispatcher's stake on `dp` to that reaction,
/// which needs one on its source anyway: queued, or, when it would be
/// the only job pending, left to run next in this job. A list that is
/// one parked async frame is settled the same way ([`frame::settle`]).
/// Anything else goes through [`settle_handler_return`] and keeps the
/// stake.
#[inline(always)]
unsafe fn settle_into_chain(dp: *mut Promise, ret_repr: u8, value: i64) -> Stake {
    unsafe {
        let link = (*dp).callbacks as usize;
        // an empty list or a heap record is one bit test, before any
        // work on the link
        if link & CB_INLINE_TAG == 0 {
            if link & CB_FRAME_TAG != 0 {
                return frame::settle(dp, link as *mut _, ret_repr, value);
            }
            settle_handler_return(dp as *mut c_void, ret_repr, value);
            return Stake::Kept;
        }
        let next = (link & !CB_INLINE_TAG) as *mut Promise;
        if !(*next).rx_next.is_null()
            || matches!(ret_repr, REPR_HEAP | REPR_ANY)
            || (*dp).state != STATE_PENDING
        {
            settle_handler_return(dp as *mut c_void, ret_repr, value);
            return Stake::Kept;
        }
        if reject_on_pending_throw(dp as *mut c_void) {
            return Stake::Kept;
        }
        (*dp).value_repr = ret_repr;
        (*dp).value_is_heap = (ret_repr == REPR_STR) as u8;
        (*dp).state = STATE_FULFILLED;
        (*dp).value = value;
        (*dp).callbacks = ptr::null_mut();
        (*next).rx_next = ptr::null_mut();
        if queue_idle() {
            Stake::RunNext(next)
        } else {
            __torajs_microtask_enqueue(inline_reaction_dispatch, next as i64);
            Stake::Queued
        }
    }
}
