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
//!   after settling.
//! - `rx_cb` with [`RX_CLOSURE`] holds one reference on the env,
//!   released after the call.
//!
//! A derived cell dying with its reaction unfired (its source died
//! first, or the cycle collector tore down a ring through it)
//! releases the env in `pool::__torajs_promise_drop`; a queued one
//! cannot die, the queue holds it.

use core::ffi::c_void;
use core::ptr;

use crate::layout::{
    Promise, REPR_VOID, RX_CATCH, RX_CLOSURE, RX_INTEREST_MASK, RX_NONE, RX_PARAM_ANY, RX_THEN,
    STATE_PENDING, STATE_REJECTED, as_promise, inline_link,
};
use crate::pool::__torajs_promise_drop;
use crate::state::{__torajs_promise_reject, __torajs_promise_resolve};
use crate::then::{ThenCbI64, call_handler, stamp_result_repr};
use crate::then_box::{settle_handler_return, settle_param};
use torajs_rc::rc_inc_cell;

pub(crate) mod candidate;
pub mod record;

unsafe extern "C" {
    fn __torajs_value_drop_heap(p: *mut c_void);
    fn __torajs_microtask_enqueue(fn_: crate::layout::MicrotaskFn, arg: i64);
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

/// PromiseReactionJob for an inline reaction: `arg` is the derived
/// cell. The reaction words are taken out of the cell before the
/// handler runs — a handler may attach to the derived promise or
/// release the last outside reference to either cell, and the cell
/// must already read as reaction-free when the stakes come off.
pub(crate) unsafe extern "C" fn inline_reaction_dispatch(arg: i64) {
    let dp = arg as *mut Promise;
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
        let fires = match kind & RX_INTEREST_MASK {
            RX_THEN => !rejected,
            RX_CATCH => rejected,
            _ => false,
        };
        if fires {
            let v = settle_param(
                (*sp).value_repr,
                kind & RX_PARAM_ANY != 0,
                param_repr,
                (*sp).value,
            );
            let result = if kind & RX_CLOSURE != 0 {
                call_handler(cb, v)
            } else {
                let f: ThenCbI64 = core::mem::transmute(cb);
                f(v)
            };
            // A Void-ret handler leaves garbage in the return
            // register — settle a clean 0 behind the VOID stamp.
            let result = if ret_repr == REPR_VOID { 0 } else { result };
            settle_handler_return(derived, ret_repr, result);
        } else {
            // The handler's interest does not match: forward the
            // settlement unchanged so a downstream handler sees it.
            stamp_result_repr(derived, (*sp).value_repr);
            if rejected {
                __torajs_promise_reject(derived, (*sp).value);
            } else {
                __torajs_promise_resolve(derived, (*sp).value);
            }
        }
        __torajs_promise_drop(source);
        if kind & RX_CLOSURE != 0 {
            __torajs_value_drop_heap(cb);
        }
        __torajs_promise_drop(derived);
    }
}
