//! `.then` / `.catch` / `.finally` runtime helpers — 6 variants
//! covering the simple-fn vs capturing-closure split per handler kind.
//!
//! Port of `runtime_promise.c` T-15.g.3, T-15.g.5, T-19.k, T-19.l,
//! T-19.n sections (P6.1, 2026-05-24). Each variant:
//!
//! 1. Allocates a fresh PENDING result Promise.
//! 2. Allocates a small heap-arg struct holding `{source, cb_or_env,
//!    result}`.
//! 3. Inc's source rc (and env rc for closure variants) so the
//!    dispatcher's `source->value` read is safe across the microtask
//!    delay.
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

use crate::layout::{STATE_FULFILLED, STATE_REJECTED, as_promise};
use crate::pool::{__torajs_promise_alloc_pending, __torajs_promise_drop};
use crate::state::{
    __torajs_promise_attach_then, __torajs_promise_reject, __torajs_promise_resolve,
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
}

unsafe extern "C" fn then_simple_dispatch(arg: i64) {
    let a = arg as *mut ThenSimpleArg;
    unsafe {
        let src = as_promise((*a).source);
        // T-19.l — `.then(onOk)` is FULFILLED-only. REJECTED forwards
        // the rejection so a downstream `.catch` picks it up.
        if (*src).state == STATE_REJECTED {
            __torajs_promise_reject((*a).result, (*src).value);
        } else {
            let result = ((*a).cb)((*src).value);
            __torajs_promise_resolve((*a).result, result);
        }
        __torajs_promise_drop((*a).source);
        free(a as *mut c_void);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_then_simple(
    source: *mut c_void,
    cb: Option<ThenCbI64>,
) -> *mut c_void {
    if source.is_null() {
        return ptr::null_mut();
    }
    let Some(cb) = cb else { return ptr::null_mut() };
    let result = unsafe { __torajs_promise_alloc_pending() };
    let a = unsafe { malloc(core::mem::size_of::<ThenSimpleArg>()) } as *mut ThenSimpleArg;
    unsafe {
        (*a).source = source;
        (*a).cb = cb;
        (*a).result = result;
        __torajs_rc_inc(source);
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
}

unsafe extern "C" fn then_closure_dispatch(arg: i64) {
    let a = arg as *mut ThenClosureArg;
    unsafe {
        let src = as_promise((*a).source);
        if (*src).state == STATE_REJECTED {
            __torajs_promise_reject((*a).result, (*src).value);
            __torajs_promise_drop((*a).source);
            __torajs_value_drop_heap((*a).env);
            free(a as *mut c_void);
            return;
        }
        let value = (*src).value;
        // Load fn_addr from env+8, call cb(env, value).
        let fn_ptr = *(((*a).env as *mut u8).add(8) as *const *mut c_void);
        let cb: ThenClosureFn = core::mem::transmute(fn_ptr);
        let result = cb((*a).env, value);
        __torajs_promise_resolve((*a).result, result);
        __torajs_promise_drop((*a).source);
        // Release the closure env ref inc'd at attach_then time.
        __torajs_value_drop_heap((*a).env);
        free(a as *mut c_void);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_then_closure(
    source: *mut c_void,
    env: *mut c_void,
) -> *mut c_void {
    if source.is_null() || env.is_null() {
        return ptr::null_mut();
    }
    let result = unsafe { __torajs_promise_alloc_pending() };
    let a = unsafe { malloc(core::mem::size_of::<ThenClosureArg>()) } as *mut ThenClosureArg;
    unsafe {
        (*a).source = source;
        (*a).env = env;
        (*a).result = result;
        __torajs_rc_inc(source);
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
}

unsafe extern "C" fn catch_simple_dispatch(arg: i64) {
    let a = arg as *mut CatchSimpleArg;
    unsafe {
        let src = as_promise((*a).source);
        if (*src).state == STATE_REJECTED {
            let result = ((*a).cb)((*src).value);
            __torajs_promise_resolve((*a).result, result);
        } else {
            __torajs_promise_resolve((*a).result, (*src).value);
        }
        __torajs_promise_drop((*a).source);
        free(a as *mut c_void);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_catch_simple(
    source: *mut c_void,
    cb: Option<ThenCbI64>,
) -> *mut c_void {
    if source.is_null() {
        return ptr::null_mut();
    }
    let Some(cb) = cb else { return ptr::null_mut() };
    let result = unsafe { __torajs_promise_alloc_pending() };
    let a = unsafe { malloc(core::mem::size_of::<CatchSimpleArg>()) } as *mut CatchSimpleArg;
    unsafe {
        (*a).source = source;
        (*a).cb = cb;
        (*a).result = result;
        __torajs_rc_inc(source);
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
}

unsafe extern "C" fn catch_closure_dispatch(arg: i64) {
    let a = arg as *mut CatchClosureArg;
    unsafe {
        let src = as_promise((*a).source);
        if (*src).state == STATE_REJECTED {
            let fn_ptr = *(((*a).env as *mut u8).add(8) as *const *mut c_void);
            let cb: ThenClosureFn = core::mem::transmute(fn_ptr);
            let result = cb((*a).env, (*src).value);
            __torajs_promise_resolve((*a).result, result);
        } else {
            __torajs_promise_resolve((*a).result, (*src).value);
        }
        __torajs_promise_drop((*a).source);
        __torajs_value_drop_heap((*a).env);
        free(a as *mut c_void);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_catch_closure(
    source: *mut c_void,
    env: *mut c_void,
) -> *mut c_void {
    if source.is_null() || env.is_null() {
        return ptr::null_mut();
    }
    let result = unsafe { __torajs_promise_alloc_pending() };
    let a = unsafe { malloc(core::mem::size_of::<CatchClosureArg>()) } as *mut CatchClosureArg;
    unsafe {
        (*a).source = source;
        (*a).env = env;
        (*a).result = result;
        __torajs_rc_inc(source);
        __torajs_rc_inc(env);
        __torajs_promise_attach_then(source, Some(catch_closure_dispatch), a as i64);
    }
    result
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
        if (*src).state == STATE_FULFILLED {
            __torajs_promise_resolve((*a).result, (*src).value);
        } else {
            // REJECTED — finally re-rejects with same reason via the
            // proper reject path so any .catch on `result` drains.
            __torajs_promise_reject((*a).result, (*src).value);
        }
        __torajs_promise_drop((*a).source);
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
    let a = unsafe { malloc(core::mem::size_of::<FinallyArg>()) } as *mut FinallyArg;
    unsafe {
        (*a).source = source;
        (*a).cb = cb;
        (*a).result = result;
        __torajs_rc_inc(source);
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
        if (*src).state == STATE_FULFILLED {
            __torajs_promise_resolve((*a).result, (*src).value);
        } else {
            __torajs_promise_reject((*a).result, (*src).value);
        }
        __torajs_promise_drop((*a).source);
        __torajs_value_drop_heap((*a).env);
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
    let a = unsafe { malloc(core::mem::size_of::<FinallyClosureArg>()) } as *mut FinallyClosureArg;
    unsafe {
        (*a).source = source;
        (*a).env = env;
        (*a).result = result;
        __torajs_rc_inc(source);
        __torajs_rc_inc(env);
        __torajs_promise_attach_then(source, Some(finally_closure_dispatch), a as i64);
    }
    result
}
