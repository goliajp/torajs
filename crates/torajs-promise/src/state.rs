//! Promise state transitions + value extraction + cb chain
//! attachment.
//!
//! Port of `runtime_promise.c` T-15.b/d sections (P6.1, 2026-05-24).
//! These three areas tightly couple:
//!
//! - `resolve` / `reject` — move a PENDING Promise to FULFILLED /
//!   REJECTED and drain its callback list onto the microtask queue.
//!   Per ES2015 the first resolve/reject wins; subsequent calls are
//!   silent no-ops.
//! - `get_value` / `get_state` — read resolved value per spec await
//!   semantics. Rejected → routes through the throw substrate
//!   (`__torajs_throw_set`) so `await rejected` propagates to the
//!   innermost try/catch.
//! - `attach_then` — append a callback to a source Promise's chain.
//!   Already-settled source → enqueue immediately; pending → append
//!   to head of chain. Drain happens lazily on transition.

use core::ffi::c_void;
use core::ptr;

use crate::layout::{
    MicrotaskFn, Promise, PromiseCb, STATE_FULFILLED, STATE_PENDING, STATE_REJECTED,
    THROW_TAG_ANY_HEAP, THROW_TAG_I64, as_promise,
};

unsafe extern "C" {
    fn malloc(n: usize) -> *mut c_void;
    fn free(p: *mut c_void);

    /// Microtask queue (libtorajs_microtask.a). Pushed by
    /// `drain_callbacks` for each cb node when a Promise settles;
    /// pushed by `attach_then`'s fast path when source is already
    /// settled at attach time.
    fn __torajs_microtask_enqueue(fn_: MicrotaskFn, arg: i64);

    /// Throw substrate (libtorajs_throw.a) — sets the per-thread
    /// throw slot so the next emit_throw_check after `get_value`'s
    /// rejected path propagates the throw to the active try/catch.
    fn __torajs_throw_set(tag: i64, value: i64);
}

/// Walk + free a Promise's cb chain, enqueuing each into the
/// microtask queue as we go. The queue copies (fn, arg) by value, so
/// the nodes themselves are transient — drain frees as it goes.
pub(crate) unsafe fn drain_callbacks(pp: *mut Promise) {
    let mut node = unsafe { (*pp).callbacks };
    while !node.is_null() {
        unsafe {
            __torajs_microtask_enqueue((*node).invoke, (*node).arg);
            let next = (*node).next;
            free(node as *mut c_void);
            node = next;
        }
    }
    unsafe { (*pp).callbacks = ptr::null_mut() };
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_resolve(p: *mut c_void, value: i64) {
    if p.is_null() {
        return;
    }
    let pp = as_promise(p);
    unsafe {
        if (*pp).state != STATE_PENDING {
            return;
        }
        (*pp).state = STATE_FULFILLED;
        (*pp).value = value;
        drain_callbacks(pp);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_reject(p: *mut c_void, reason: i64) {
    if p.is_null() {
        return;
    }
    let pp = as_promise(p);
    unsafe {
        if (*pp).state != STATE_PENDING {
            return;
        }
        (*pp).state = STATE_REJECTED;
        (*pp).value = reason;
        drain_callbacks(pp);
    }
}

/// `await p` value extraction:
///   - FULFILLED → return value (raw i64 — heap ptrs returned as bits)
///   - REJECTED  → route through `__torajs_throw_set` + return 0;
///     emit_throw_check after the call sees the throw slot non-empty
///     and propagates.
///   - PENDING   → return 0 (sync-resolve model; the silent 0 guards
///     against crashes pre-event-loop).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_get_value(p: *const c_void) -> i64 {
    if p.is_null() {
        return 0;
    }
    let pp = p as *const Promise;
    let state = unsafe { (*pp).state };
    if state == STATE_REJECTED {
        let tag = if unsafe { (*pp).value_is_heap } != 0 {
            THROW_TAG_ANY_HEAP
        } else {
            THROW_TAG_I64
        };
        unsafe { __torajs_throw_set(tag, (*pp).value) };
        return 0;
    }
    if state != STATE_FULFILLED {
        return 0;
    }
    unsafe { (*pp).value }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_get_state(p: *const c_void) -> u8 {
    if p.is_null() {
        return STATE_PENDING;
    }
    let pp = p as *const Promise;
    unsafe { (*pp).state }
}

/// `.then` runtime hook — append a callback to the source Promise's
/// chain. Two timing paths:
///   1. source already settled → enqueue immediately.
///   2. source pending → head-push onto callbacks list; resolve /
///      reject drains the list lazily.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_attach_then(
    source_p: *mut c_void,
    invoke: Option<MicrotaskFn>,
    arg: i64,
) {
    if source_p.is_null() {
        return;
    }
    let Some(invoke) = invoke else { return };
    let pp = as_promise(source_p);
    let state = unsafe { (*pp).state };
    if state != STATE_PENDING {
        // Already settled — enqueue immediately.
        unsafe { __torajs_microtask_enqueue(invoke, arg) };
        return;
    }
    let node = unsafe { malloc(core::mem::size_of::<PromiseCb>()) } as *mut PromiseCb;
    unsafe {
        (*node).invoke = invoke;
        (*node).arg = arg;
        (*node).next = (*pp).callbacks;
        (*pp).callbacks = node;
    }
}
