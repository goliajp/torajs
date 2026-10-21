//! `.then` / `.catch` runtime helpers — 4 variants covering the
//! simple-fn vs capturing-closure split per handler kind (the
//! `.finally` pair lives in the sibling [`crate::then_finally`]).
//!
//! Each variant mints a fresh PENDING result Promise and parks the
//! reaction `{source, cb_or_env, reprs}` INSIDE it — the inline form
//! in [`crate::reaction`], one allocation per `then`. The stakes the
//! reaction holds (source, result, closure env) and why the result
//! needs one of its own are documented there; the dispatcher reads
//! the source's state, runs the handler when the state matches its
//! interest (`.then` fires on FULFILLED; `.catch` on REJECTED;
//! `.finally` always fires + propagates state unchanged), settles
//! the result and releases the stakes.
//!
//! Closure variants assume the env layout from ssa_lower's
//! CLOSURE_*_OFF constants:
//!
//! ```text
//!   env+0   : universal heap header
//!   env+8   : fn_addr
//!   env+16  : drop_fn ptr
//!   env+24+ : capture slots
//! ```

use core::ffi::c_void;
use core::ptr;

use crate::layout::{RX_CATCH, RX_CLOSURE, RX_PARAM_ANY, RX_THEN, STATE_PENDING, as_promise};
use crate::pool::{alloc_pending, pop_pending};
use crate::reaction::{attach_inline, attach_plain_pending, attach_plain_settled};
use crate::then_box::{PARAM_ANY_FLAG, PARAM_REPR_SHIFT, refuse_unstamped};

/// RFC 20260720-anylane-promise-methods knife 3 — the settle path
/// fixes the result cell's value form for the any-lane bridge: a
/// callback leg stamps the call site's static return repr, a
/// forward leg copies the source's stamp (UNSTAMPED propagates as
/// UNSTAMPED — still loud, never a mis-box).
pub(crate) unsafe fn stamp_result_repr(result: *mut c_void, repr: u8) {
    unsafe { (*as_promise(result)).value_repr = repr };
}

/// User-fn signature for the i64→i64 simple variants
/// (`.then(cb)` / `.catch(cb)`).
pub type ThenCbI64 = unsafe extern "C" fn(i64) -> i64;

/// User-fn signature for the closure variants. First param is the
/// closure env block (the body uses it to load captures); second the
/// hidden argc (S1, RFC 20260810-indirect-argc-abi — every
/// `__env`-first entry carries it right after the env; a then/catch
/// handler receives exactly one argument); third the source's
/// resolved value.
pub type ThenClosureFn = unsafe extern "C" fn(*mut c_void, i64, i64) -> i64;

/// The receiver-first flavor of [`ThenClosureFn`]: a cell stamped
/// `FLAG_CLOSURE_RECV_FIRST` takes its boxed `__this` between the
/// hidden argc and the settled value (argc still counts the user
/// arguments only — the S1 spec count).
pub type ThenRecvClosureFn = unsafe extern "C" fn(*mut c_void, i64, i64, i64) -> i64;

/// Call a closure handler with the settled value. §27.2.5.4.1 runs a
/// then / catch handler with an undefined `this`, so a receiver-first
/// cell (RFC 20260905 residue 1 — the handler was one of the lanes
/// that did not read the flag and let the value land in `__this`)
/// gets the undefined box as its leading argument; a plain cell keeps
/// the two-word face. `env` must be a live closure cell.
pub(crate) unsafe fn call_handler(env: *mut c_void, value: i64) -> i64 {
    unsafe {
        let fn_ptr = crate::closure_abi::fn_ptr(env);
        if crate::closure_abi::wants_receiver(env) {
            let cb: ThenRecvClosureFn = core::mem::transmute(fn_ptr);
            cb(env, 1, crate::closure_abi::undefined_box(), value)
        } else {
            let cb: ThenClosureFn = core::mem::transmute(fn_ptr);
            cb(env, 1, value)
        }
    }
}

/// `.finally(cb)` simple — no value in. §27.2.5.3 runs the handler
/// argument-free but USES what it returns (a thenable is waited on),
/// so the return rides back like the then/catch pair's, with the call
/// site's ret-repr word saying whether the register holds anything.
pub type FinallyCb = unsafe extern "C" fn() -> i64;

/// `.finally(cb)` closure — env + hidden argc in (§27.2.5.3 runs the
/// handler argument-free, so argc is 0), return as above.
pub type FinallyClosureFn = unsafe extern "C" fn(*mut c_void, i64) -> i64;

/// The receiver-first face of [`FinallyClosureFn`] — §27.2.5.3 runs
/// the handler with an undefined `this`, boxed into the leading word.
pub type FinallyRecvClosureFn = unsafe extern "C" fn(*mut c_void, i64, i64) -> i64;

/// The four typed kernels share one body: validate, mint the derived
/// cell pre-stamped with the call site's return repr (a chained
/// attach lands before this cell settles and its any-lane gate must
/// see a known form; the dispatcher overwrites on the forward leg),
/// then park the reaction inline. `cb` is a fn pointer or a closure
/// env per `kind`'s [`RX_CLOSURE`] bit.
///
/// A plain handler with a block on the row is inlined into the entry
/// points without a frame: on a pending source — every link while a
/// `then` chain is built — it makes no call at all, and on a settled
/// one its only call, the queue push, is a tail call. The rest goes to
/// [`then_kernel_full`].
#[inline(always)]
unsafe fn then_kernel(
    source: *mut c_void,
    cb: *mut c_void,
    kind: u8,
    ret_repr: i64,
) -> *mut c_void {
    unsafe {
        let sp = as_promise(source);
        if ret_repr & PARAM_ANY_FLAG == 0 && kind & RX_CLOSURE == 0 {
            let param_repr = ((ret_repr >> PARAM_REPR_SHIFT) & 0xff) as u8;
            let ret = (ret_repr & 0xff) as u8;
            // one pop per arm: the state is read before the new cell is
            // written, so each arm's fresh fields and reaction fold into
            // one set of stores
            if (*sp).state == STATE_PENDING {
                let result = pop_pending();
                if !result.is_null() {
                    stamp_result_repr(result, ret);
                    attach_plain_pending(sp, as_promise(result), kind, cb, ret, param_repr);
                    return result;
                }
            } else {
                let result = pop_pending();
                if !result.is_null() {
                    stamp_result_repr(result, ret);
                    return attach_plain_settled(sp, as_promise(result), kind, cb, ret, param_repr);
                }
            }
        }
        then_kernel_full(source, cb, kind, ret_repr)
    }
}

#[inline(never)]
unsafe fn then_kernel_full(
    source: *mut c_void,
    cb: *mut c_void,
    kind: u8,
    ret_repr: i64,
) -> *mut c_void {
    let param_any = (ret_repr & PARAM_ANY_FLAG) != 0;
    let param_repr = ((ret_repr >> PARAM_REPR_SHIFT) & 0xff) as u8;
    if param_any && unsafe { refuse_unstamped(source) } {
        return ptr::null_mut();
    }
    let ret_repr = (ret_repr & 0xff) as u8;
    let kind = if param_any { kind | RX_PARAM_ANY } else { kind };
    let result = unsafe { alloc_pending() };
    unsafe {
        stamp_result_repr(result, ret_repr);
        attach_inline(source, result, kind, cb, ret_repr, param_repr);
    }
    result
}

/// `.then(cb)` with `cb: (v: i64) -> i64`.
///
/// # Safety
/// `source` is NULL or a live Promise cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_then_simple(
    source: *mut c_void,
    cb: Option<ThenCbI64>,
    ret_repr: i64,
) -> *mut c_void {
    if source.is_null() {
        return ptr::null_mut();
    }
    let Some(cb) = cb else { return ptr::null_mut() };
    unsafe { then_kernel(source, cb as *mut c_void, RX_THEN, ret_repr) }
}

/// `.then(closure)` — `env` is the closure cell (`call_handler`).
///
/// # Safety
/// `source` is NULL or a live Promise cell; `env` is NULL or a live closure env cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_then_closure(
    source: *mut c_void,
    env: *mut c_void,
    ret_repr: i64,
) -> *mut c_void {
    if source.is_null() || env.is_null() {
        return ptr::null_mut();
    }
    unsafe { then_kernel(source, env, RX_THEN | RX_CLOSURE, ret_repr) }
}

/// `.catch(cb)` with `cb: (reason: i64) -> i64`; only fires on
/// REJECTED, forwards FULFILLED.
///
/// # Safety
/// `source` is NULL or a live Promise cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_catch_simple(
    source: *mut c_void,
    cb: Option<ThenCbI64>,
    ret_repr: i64,
) -> *mut c_void {
    if source.is_null() {
        return ptr::null_mut();
    }
    let Some(cb) = cb else { return ptr::null_mut() };
    unsafe { then_kernel(source, cb as *mut c_void, RX_CATCH, ret_repr) }
}

/// `.catch(closure)`.
///
/// # Safety
/// `source` is NULL or a live Promise cell; `env` is NULL or a live closure env cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_catch_closure(
    source: *mut c_void,
    env: *mut c_void,
    ret_repr: i64,
) -> *mut c_void {
    if source.is_null() || env.is_null() {
        return ptr::null_mut();
    }
    unsafe { then_kernel(source, env, RX_CATCH | RX_CLOSURE, ret_repr) }
}

// `.finally` simple + closure variants live in the sibling
// `then_finally.rs` (file-size split, RFC 20260720-promise-any-cb).
