//! `.finally` runtime helpers — the simple + closure variants,
//! verbatim from [`crate::then`] (RFC 20260720-promise-any-cb: the
//! result-stake fix pushed that file past the 500-line cap; the
//! finally pair is the natural cut).
//!
//! §27.2.5.3 runs the handler argument-free, and until rotation 301
//! that was read as "and ignores what it returns" — the callback type
//! was `fn()`, so the return value never left the register and a
//! handler declared to return anything at all was a COMPILE reject.
//! The spec uses it: a thenable is waited on before the source's
//! settlement is forwarded, which is what makes
//! `.finally(() => cleanupAsync())` mean anything. So the return
//! rides back like the then/catch pair's, behind the call site's
//! ret-repr word (a `void` handler leaves garbage in the register —
//! the word is what says so).
//!
//! What the result settles with is still the SOURCE's settlement, not
//! the handler's value. The one thing that displaces it is a returned
//! promise that REJECTS: §27.2.5.3 builds `promiseResolve(C, result)
//! .then(() => value)`, whose onFulfilled-only shape lets that
//! rejection through on either leg. Verified against bun.

use core::ffi::c_void;
use core::ptr;

use crate::layout::{REPR_ANY, REPR_HEAP, REPR_STR, REPR_VOID, STATE_FULFILLED, as_promise};
use crate::pool::{__torajs_promise_alloc_pending, __torajs_promise_drop};
use crate::state::{
    __torajs_promise_attach_then, __torajs_promise_reject, __torajs_promise_resolve,
};
use crate::then::{FinallyCb, FinallyClosureFn, stamp_result_repr};
use crate::then_box::reject_on_pending_throw;

unsafe extern "C" {
    /// torajs-mmalloc libc-compat — v0.7-A2 step 6b cutover.
    #[link_name = "__torajs_libc_malloc"]
    fn malloc(n: usize) -> *mut c_void;
    #[link_name = "__torajs_libc_free"]
    fn free(p: *mut c_void);
    fn __torajs_rc_inc(p: *mut c_void);
    fn __torajs_value_drop_heap(p: *mut c_void);
}

/// Forward the source's own settlement — what `.finally` answers with
/// whatever the handler did.
unsafe fn forward_source(source: *mut c_void, result: *mut c_void) {
    unsafe {
        let src = as_promise(source);
        stamp_result_repr(result, (*src).value_repr);
        if (*src).state == STATE_FULFILLED {
            __torajs_promise_resolve(result, (*src).value);
        } else {
            // REJECTED — finally re-rejects with same reason via the
            // proper reject path so any .catch on `result` drains.
            __torajs_promise_reject(result, (*src).value);
        }
    }
}

/// Release a handler return nobody keeps. The value is owned (the
/// then/catch kernels hand theirs straight to `resolve`, which stores
/// without an inc), so a discarded one has to be let go or it strands.
unsafe fn drop_handler_return(ret_repr: u8, ret: i64) {
    if ret == 0 {
        return;
    }
    if matches!(ret_repr, REPR_STR | REPR_HEAP | REPR_ANY) {
        // NaN-box aware, so immediates in an `any` return pass through.
        unsafe { __torajs_value_drop_heap(ret as *mut c_void) };
    }
}

/// The waited-on promise settled — §27.2.5.3's `.then(() => value)`.
#[repr(C)]
struct FinallyWaitArg {
    source: *mut c_void,
    inner: *mut c_void,
    result: *mut c_void,
}

unsafe extern "C" fn finally_wait_dispatch(arg: i64) {
    let a = arg as *mut FinallyWaitArg;
    unsafe {
        let ip = as_promise((*a).inner);
        if (*ip).state == STATE_FULFILLED {
            forward_source((*a).source, (*a).result);
        } else {
            // The `.then(() => value)` §27.2.5.3 builds has no
            // onRejected, so a rejected wait displaces the source's
            // settlement on either leg.
            //
            // This leg takes a stake where the source leg above does
            // not: the inner promise is one the handler made and this
            // job is about to drop, so it may be the only owner of
            // its reason.
            let reason = (*ip).value;
            let rp = as_promise((*a).result);
            (*rp).value_repr = (*ip).value_repr;
            (*rp).value_is_heap = (*ip).value_is_heap;
            if (*ip).value_is_heap != 0 && reason != 0 {
                __torajs_rc_inc(reason as *mut c_void);
            }
            __torajs_promise_reject((*a).result, reason);
        }
        __torajs_promise_drop((*a).source);
        __torajs_promise_drop((*a).inner);
        __torajs_promise_drop((*a).result);
        free(a as *mut c_void);
    }
}

/// The tail both variants share: decide what the handler's return
/// means and settle (or arrange to settle) the result.
///
/// Consumes one stake each on `source` and `result`, and the
/// handler's stake on `ret`.
unsafe fn finish(source: *mut c_void, result: *mut c_void, ret_repr: u8, ret: i64) {
    unsafe {
        // §27.2.5.3 — onFinally throwing WINS over the settlement it
        // was about to forward, on either leg. A throwing call left
        // the return register undefined, so `ret` is not read here.
        if reject_on_pending_throw(result) {
            __torajs_promise_drop(source);
            __torajs_promise_drop(result);
            return;
        }
        if let Some(inner) = crate::then_adopt::returned_promise(ret_repr, ret) {
            let w = malloc(core::mem::size_of::<FinallyWaitArg>()) as *mut FinallyWaitArg;
            (*w).source = source;
            (*w).inner = inner;
            (*w).result = result;
            // All three stakes move into the waiting job: the two
            // this call was handed, and the handler's on what it
            // returned.
            __torajs_promise_attach_then(inner, Some(finally_wait_dispatch), w as i64);
            return;
        }
        drop_handler_return(ret_repr, ret);
        forward_source(source, result);
        __torajs_promise_drop(source);
        __torajs_promise_drop(result);
    }
}

/// A `void` handler leaves garbage in the return register, so the
/// call site's word is the only thing that can say whether there is a
/// value at all.
#[inline]
fn returned_value(ret_repr: u8, raw: i64) -> i64 {
    if ret_repr == REPR_VOID { 0 } else { raw }
}

// ============================================================
// .finally simple — cb: () -> any; fires on both fulfilled & rejected
// ============================================================

#[repr(C)]
struct FinallyArg {
    source: *mut c_void,
    cb: FinallyCb,
    result: *mut c_void,
    ret_repr: u8,
}

unsafe extern "C" fn finally_dispatch(arg: i64) {
    let a = arg as *mut FinallyArg;
    unsafe {
        let raw = ((*a).cb)();
        let ret = returned_value((*a).ret_repr, raw);
        finish((*a).source, (*a).result, (*a).ret_repr, ret);
        free(a as *mut c_void);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_finally(
    source: *mut c_void,
    cb: Option<FinallyCb>,
    ret_repr: i64,
) -> *mut c_void {
    if source.is_null() {
        return ptr::null_mut();
    }
    let Some(cb) = cb else { return ptr::null_mut() };
    let result = unsafe { __torajs_promise_alloc_pending() };
    // Pre-stamp from the source — finally forwards the settlement,
    // so the source's current form is the best attach-time answer
    // (the dispatcher re-copies after the source settles).
    unsafe { stamp_result_repr(result, (*as_promise(source)).value_repr) };
    let a = unsafe { malloc(core::mem::size_of::<FinallyArg>()) } as *mut FinallyArg;
    unsafe {
        (*a).source = source;
        (*a).cb = cb;
        (*a).result = result;
        (*a).ret_repr = ret_repr as u8;
        __torajs_rc_inc(source);
        __torajs_rc_inc(result);
        __torajs_promise_attach_then(source, Some(finally_dispatch), a as i64);
    }
    result
}

// ============================================================
// .finally closure
// ============================================================

#[repr(C)]
struct FinallyClosureArg {
    source: *mut c_void,
    env: *mut c_void,
    result: *mut c_void,
    ret_repr: u8,
}

unsafe extern "C" fn finally_closure_dispatch(arg: i64) {
    let a = arg as *mut FinallyClosureArg;
    unsafe {
        let fn_ptr = *(((*a).env as *mut u8).add(8) as *const *mut c_void);
        let cb: FinallyClosureFn = core::mem::transmute(fn_ptr);
        let raw = cb((*a).env);
        let ret = returned_value((*a).ret_repr, raw);
        // The env dies with the job either way; `finish` may hand the
        // rest on to a waiting job.
        __torajs_value_drop_heap((*a).env);
        finish((*a).source, (*a).result, (*a).ret_repr, ret);
        free(a as *mut c_void);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_finally_closure(
    source: *mut c_void,
    env: *mut c_void,
    ret_repr: i64,
) -> *mut c_void {
    if source.is_null() || env.is_null() {
        return ptr::null_mut();
    }
    let result = unsafe { __torajs_promise_alloc_pending() };
    // Pre-stamp from the source — finally forwards the settlement,
    // so the source's current form is the best attach-time answer
    // (the dispatcher re-copies after the source settles).
    unsafe { stamp_result_repr(result, (*as_promise(source)).value_repr) };
    let a = unsafe { malloc(core::mem::size_of::<FinallyClosureArg>()) } as *mut FinallyClosureArg;
    unsafe {
        (*a).source = source;
        (*a).env = env;
        (*a).result = result;
        (*a).ret_repr = ret_repr as u8;
        __torajs_rc_inc(source);
        __torajs_rc_inc(result);
        __torajs_rc_inc(env);
        __torajs_promise_attach_then(source, Some(finally_closure_dispatch), a as i64);
    }
    result
}
