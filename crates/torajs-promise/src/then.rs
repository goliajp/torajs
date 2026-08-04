//! `.then` / `.catch` runtime helpers — 4 variants covering the
//! simple-fn vs capturing-closure split per handler kind (the
//! `.finally` pair lives in the sibling [`crate::then_finally`]).
//!
//! Port of `runtime_promise.c` T-15.g.3, T-15.g.5, T-19.k, T-19.l,
//! T-19.n sections (P6.1, 2026-05-24). Each variant:
//!
//! 1. Allocates a fresh PENDING result Promise.
//! 2. Allocates a small heap-arg struct holding `{source, cb_or_env,
//!    result}`.
//! 3. Inc's source rc (and env rc for closure variants) so the
//!    dispatcher's `source->value` read is safe across the microtask
//!    delay — and the RESULT rc too (RFC 20260720-promise-any-cb):
//!    a discarding call site (`p.then(cb);` as a statement) drops
//!    the returned promise before the microtask fires, and without
//!    its own stake the dispatcher would settle a pool-recycled
//!    cell (the repr pre-stamp corrupted whatever promise the pool
//!    handed out next; state/value only survived by the settled
//!    guard). The dispatcher releases the stake after settling —
//!    the same contract the any-lane bridge's ThenAnyArg carries.
//! 4. Calls `attach_then(source, dispatcher, &arg)`.
//! 5. Returns the result Promise.
//!
//! Dispatchers (one per variant) read source state, invoke cb if
//! state matches the handler's interest (`.then` fires on FULFILLED;
//! `.catch` on REJECTED; `.finally` always fires + propagates state
//! unchanged), resolve/reject the result, dec source rc + free heap
//! args.
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

use crate::layout::{REPR_VOID, STATE_REJECTED, as_promise};
use crate::pool::{__torajs_promise_alloc_pending, __torajs_promise_drop};
use crate::state::{
    __torajs_promise_attach_then, __torajs_promise_reject, __torajs_promise_resolve,
};
use crate::then_box::{
    PARAM_ANY_FLAG, PARAM_REPR_SHIFT, refuse_unstamped, settle_handler_return, settle_param,
};

unsafe extern "C" {
    /// torajs-mmalloc libc-compat — v0.7-A2 step 6b cutover.
    #[link_name = "__torajs_libc_malloc"]
    fn malloc(n: usize) -> *mut c_void;
    #[link_name = "__torajs_libc_free"]
    fn free(p: *mut c_void);
    fn __torajs_rc_inc(p: *mut c_void);
    fn __torajs_value_drop_heap(p: *mut c_void);
}

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
/// closure env block (the body uses it to load captures); second is
/// the source's resolved value.
pub type ThenClosureFn = unsafe extern "C" fn(*mut c_void, i64) -> i64;

/// `.finally(cb)` simple — no value in, no return.
pub type FinallyCb = unsafe extern "C" fn();

/// `.finally(cb)` closure — env in, no return.
pub type FinallyClosureFn = unsafe extern "C" fn(*mut c_void);

// ============================================================
// .then simple — cb: (v: i64) -> i64
// ============================================================

#[repr(C)]
struct ThenSimpleArg {
    source: *mut c_void,
    cb: ThenCbI64,
    result: *mut c_void,
    ret_repr: u8,
    param_any: u8,
    param_repr: u8,
}

unsafe extern "C" fn then_simple_dispatch(arg: i64) {
    let a = arg as *mut ThenSimpleArg;
    unsafe {
        let src = as_promise((*a).source);
        // T-19.l — `.then(onOk)` is FULFILLED-only. REJECTED forwards
        // the rejection so a downstream `.catch` picks it up.
        if (*src).state == STATE_REJECTED {
            stamp_result_repr((*a).result, (*src).value_repr);
            __torajs_promise_reject((*a).result, (*src).value);
        } else {
            let v = settle_param(
                (*src).value_repr,
                (*a).param_any != 0,
                (*a).param_repr,
                (*src).value,
            );
            let result = ((*a).cb)(v);
            // A Void-ret handler leaves garbage in the return
            // register — settle a clean 0 behind the VOID stamp.
            let result = if (*a).ret_repr == REPR_VOID {
                0
            } else {
                result
            };
            settle_handler_return((*a).result, (*a).ret_repr, result);
        }
        __torajs_promise_drop((*a).source);
        __torajs_promise_drop((*a).result);
        free(a as *mut c_void);
    }
}

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
    let param_any = (ret_repr & PARAM_ANY_FLAG) != 0;
    let param_repr = ((ret_repr >> PARAM_REPR_SHIFT) & 0xff) as u8;
    if param_any && unsafe { refuse_unstamped(source) } {
        return ptr::null_mut();
    }
    let ret_repr = (ret_repr & 0xff) as u8;
    let result = unsafe { __torajs_promise_alloc_pending() };
    // Pre-stamp with the cb-leg form — a chained attach lands before
    // this cell settles and its any-lane gate must see a known form;
    // the dispatcher overwrites on the forward leg.
    unsafe { stamp_result_repr(result, ret_repr) };
    let a = unsafe { malloc(core::mem::size_of::<ThenSimpleArg>()) } as *mut ThenSimpleArg;
    unsafe {
        (*a).source = source;
        (*a).cb = cb;
        (*a).result = result;
        (*a).ret_repr = ret_repr;
        (*a).param_any = param_any as u8;
        (*a).param_repr = param_repr;
        __torajs_rc_inc(source);
        __torajs_rc_inc(result);
        __torajs_promise_attach_then(source, Some(then_simple_dispatch), a as i64);
    }
    result
}

// ============================================================
// .then closure — cb_body: (env*, v: i64) -> i64
// ============================================================

#[repr(C)]
struct ThenClosureArg {
    source: *mut c_void,
    env: *mut c_void,
    result: *mut c_void,
    ret_repr: u8,
    param_any: u8,
    param_repr: u8,
}

unsafe extern "C" fn then_closure_dispatch(arg: i64) {
    let a = arg as *mut ThenClosureArg;
    unsafe {
        let src = as_promise((*a).source);
        if (*src).state == STATE_REJECTED {
            stamp_result_repr((*a).result, (*src).value_repr);
            __torajs_promise_reject((*a).result, (*src).value);
            __torajs_promise_drop((*a).source);
            __torajs_value_drop_heap((*a).env);
            __torajs_promise_drop((*a).result);
            free(a as *mut c_void);
            return;
        }
        let value = settle_param(
            (*src).value_repr,
            (*a).param_any != 0,
            (*a).param_repr,
            (*src).value,
        );
        // Load fn_addr from env+8, call cb(env, value).
        let fn_ptr = *(((*a).env as *mut u8).add(8) as *const *mut c_void);
        let cb: ThenClosureFn = core::mem::transmute(fn_ptr);
        let result = cb((*a).env, value);
        // A Void-ret handler leaves garbage in the return register —
        // settle a clean 0 behind the VOID stamp.
        let result = if (*a).ret_repr == REPR_VOID {
            0
        } else {
            result
        };
        settle_handler_return((*a).result, (*a).ret_repr, result);
        __torajs_promise_drop((*a).source);
        // Release the closure env ref inc'd at attach_then time.
        __torajs_value_drop_heap((*a).env);
        __torajs_promise_drop((*a).result);
        free(a as *mut c_void);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_then_closure(
    source: *mut c_void,
    env: *mut c_void,
    ret_repr: i64,
) -> *mut c_void {
    if source.is_null() || env.is_null() {
        return ptr::null_mut();
    }
    let param_any = (ret_repr & PARAM_ANY_FLAG) != 0;
    let param_repr = ((ret_repr >> PARAM_REPR_SHIFT) & 0xff) as u8;
    if param_any && unsafe { refuse_unstamped(source) } {
        return ptr::null_mut();
    }
    let ret_repr = (ret_repr & 0xff) as u8;
    let result = unsafe { __torajs_promise_alloc_pending() };
    // Pre-stamp with the cb-leg form — a chained attach lands before
    // this cell settles and its any-lane gate must see a known form;
    // the dispatcher overwrites on the forward leg.
    unsafe { stamp_result_repr(result, ret_repr) };
    let a = unsafe { malloc(core::mem::size_of::<ThenClosureArg>()) } as *mut ThenClosureArg;
    unsafe {
        (*a).source = source;
        (*a).env = env;
        (*a).result = result;
        (*a).ret_repr = ret_repr;
        (*a).param_any = param_any as u8;
        (*a).param_repr = param_repr;
        __torajs_rc_inc(source);
        __torajs_rc_inc(result);
        __torajs_rc_inc(env);
        __torajs_promise_attach_then(source, Some(then_closure_dispatch), a as i64);
    }
    result
}

// ============================================================
// .catch simple — cb: (reason: i64) -> i64; only fires on REJECTED
// ============================================================

#[repr(C)]
struct CatchSimpleArg {
    source: *mut c_void,
    cb: ThenCbI64,
    result: *mut c_void,
    ret_repr: u8,
    param_any: u8,
    param_repr: u8,
}

unsafe extern "C" fn catch_simple_dispatch(arg: i64) {
    let a = arg as *mut CatchSimpleArg;
    unsafe {
        let src = as_promise((*a).source);
        if (*src).state == STATE_REJECTED {
            let v = settle_param(
                (*src).value_repr,
                (*a).param_any != 0,
                (*a).param_repr,
                (*src).value,
            );
            let result = ((*a).cb)(v);
            // A Void-ret handler leaves garbage in the return
            // register — settle a clean 0 behind the VOID stamp.
            let result = if (*a).ret_repr == REPR_VOID {
                0
            } else {
                result
            };
            settle_handler_return((*a).result, (*a).ret_repr, result);
        } else {
            stamp_result_repr((*a).result, (*src).value_repr);
            __torajs_promise_resolve((*a).result, (*src).value);
        }
        __torajs_promise_drop((*a).source);
        __torajs_promise_drop((*a).result);
        free(a as *mut c_void);
    }
}

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
    let param_any = (ret_repr & PARAM_ANY_FLAG) != 0;
    let param_repr = ((ret_repr >> PARAM_REPR_SHIFT) & 0xff) as u8;
    if param_any && unsafe { refuse_unstamped(source) } {
        return ptr::null_mut();
    }
    let ret_repr = (ret_repr & 0xff) as u8;
    let result = unsafe { __torajs_promise_alloc_pending() };
    // Pre-stamp with the cb-leg form — a chained attach lands before
    // this cell settles and its any-lane gate must see a known form;
    // the dispatcher overwrites on the forward leg.
    unsafe { stamp_result_repr(result, ret_repr) };
    let a = unsafe { malloc(core::mem::size_of::<CatchSimpleArg>()) } as *mut CatchSimpleArg;
    unsafe {
        (*a).source = source;
        (*a).cb = cb;
        (*a).result = result;
        (*a).ret_repr = ret_repr;
        (*a).param_any = param_any as u8;
        (*a).param_repr = param_repr;
        __torajs_rc_inc(source);
        __torajs_rc_inc(result);
        __torajs_promise_attach_then(source, Some(catch_simple_dispatch), a as i64);
    }
    result
}

// ============================================================
// .catch closure
// ============================================================

#[repr(C)]
struct CatchClosureArg {
    source: *mut c_void,
    env: *mut c_void,
    result: *mut c_void,
    ret_repr: u8,
    param_any: u8,
    param_repr: u8,
}

unsafe extern "C" fn catch_closure_dispatch(arg: i64) {
    let a = arg as *mut CatchClosureArg;
    unsafe {
        let src = as_promise((*a).source);
        if (*src).state == STATE_REJECTED {
            let v = settle_param(
                (*src).value_repr,
                (*a).param_any != 0,
                (*a).param_repr,
                (*src).value,
            );
            let fn_ptr = *(((*a).env as *mut u8).add(8) as *const *mut c_void);
            let cb: ThenClosureFn = core::mem::transmute(fn_ptr);
            let result = cb((*a).env, v);
            // A Void-ret handler leaves garbage in the return
            // register — settle a clean 0 behind the VOID stamp.
            let result = if (*a).ret_repr == REPR_VOID {
                0
            } else {
                result
            };
            settle_handler_return((*a).result, (*a).ret_repr, result);
        } else {
            stamp_result_repr((*a).result, (*src).value_repr);
            __torajs_promise_resolve((*a).result, (*src).value);
        }
        __torajs_promise_drop((*a).source);
        __torajs_value_drop_heap((*a).env);
        __torajs_promise_drop((*a).result);
        free(a as *mut c_void);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_catch_closure(
    source: *mut c_void,
    env: *mut c_void,
    ret_repr: i64,
) -> *mut c_void {
    if source.is_null() || env.is_null() {
        return ptr::null_mut();
    }
    let param_any = (ret_repr & PARAM_ANY_FLAG) != 0;
    let param_repr = ((ret_repr >> PARAM_REPR_SHIFT) & 0xff) as u8;
    if param_any && unsafe { refuse_unstamped(source) } {
        return ptr::null_mut();
    }
    let ret_repr = (ret_repr & 0xff) as u8;
    let result = unsafe { __torajs_promise_alloc_pending() };
    // Pre-stamp with the cb-leg form — a chained attach lands before
    // this cell settles and its any-lane gate must see a known form;
    // the dispatcher overwrites on the forward leg.
    unsafe { stamp_result_repr(result, ret_repr) };
    let a = unsafe { malloc(core::mem::size_of::<CatchClosureArg>()) } as *mut CatchClosureArg;
    unsafe {
        (*a).source = source;
        (*a).env = env;
        (*a).result = result;
        (*a).ret_repr = ret_repr;
        (*a).param_any = param_any as u8;
        (*a).param_repr = param_repr;
        __torajs_rc_inc(source);
        __torajs_rc_inc(result);
        __torajs_rc_inc(env);
        __torajs_promise_attach_then(source, Some(catch_closure_dispatch), a as i64);
    }
    result
}

// `.finally` simple + closure variants live in the sibling
// `then_finally.rs` (file-size split, RFC 20260720-promise-any-cb).
