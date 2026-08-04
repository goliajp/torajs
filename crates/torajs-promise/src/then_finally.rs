//! `.finally` runtime helpers — the simple + closure variants,
//! verbatim from [`crate::then`] (RFC 20260720-promise-any-cb: the
//! result-stake fix pushed that file past the 500-line cap; the
//! finally pair is the natural cut — it carries no ret-repr /
//! param-any channel, §27.2.5.3 runs the callback argument-free and
//! forwards the settlement).

use core::ffi::c_void;
use core::ptr;

use crate::layout::{STATE_FULFILLED, as_promise};
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

// ============================================================
// .finally simple — cb: () -> void; fires on both fulfilled & rejected
// ============================================================

#[repr(C)]
struct FinallyArg {
    source: *mut c_void,
    cb: FinallyCb,
    result: *mut c_void,
}

unsafe extern "C" fn finally_dispatch(arg: i64) {
    let a = arg as *mut FinallyArg;
    unsafe {
        let src = as_promise((*a).source);
        ((*a).cb)();
        // §27.2.5.3 — onFinally throwing WINS over the settlement it
        // was about to forward, on either leg.
        if !reject_on_pending_throw((*a).result) {
            stamp_result_repr((*a).result, (*src).value_repr);
            if (*src).state == STATE_FULFILLED {
                __torajs_promise_resolve((*a).result, (*src).value);
            } else {
                // REJECTED — finally re-rejects with same reason via
                // the proper reject path so any .catch on `result`
                // drains.
                __torajs_promise_reject((*a).result, (*src).value);
            }
        }
        __torajs_promise_drop((*a).source);
        __torajs_promise_drop((*a).result);
        free(a as *mut c_void);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_finally(
    source: *mut c_void,
    cb: Option<FinallyCb>,
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
}

unsafe extern "C" fn finally_closure_dispatch(arg: i64) {
    let a = arg as *mut FinallyClosureArg;
    unsafe {
        let src = as_promise((*a).source);
        let fn_ptr = *(((*a).env as *mut u8).add(8) as *const *mut c_void);
        let cb: FinallyClosureFn = core::mem::transmute(fn_ptr);
        cb((*a).env);
        if !reject_on_pending_throw((*a).result) {
            stamp_result_repr((*a).result, (*src).value_repr);
            if (*src).state == STATE_FULFILLED {
                __torajs_promise_resolve((*a).result, (*src).value);
            } else {
                __torajs_promise_reject((*a).result, (*src).value);
            }
        }
        __torajs_promise_drop((*a).source);
        __torajs_value_drop_heap((*a).env);
        __torajs_promise_drop((*a).result);
        free(a as *mut c_void);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_finally_closure(
    source: *mut c_void,
    env: *mut c_void,
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
        __torajs_rc_inc(source);
        __torajs_rc_inc(result);
        __torajs_rc_inc(env);
        __torajs_promise_attach_then(source, Some(finally_closure_dispatch), a as i64);
    }
    result
}
