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
    MicrotaskFn, Promise, PromiseCb, REPR_ANY, STATE_FULFILLED, STATE_PENDING, STATE_REJECTED,
    THROW_TAG_ANY_HEAP, THROW_TAG_I64, as_promise,
};

unsafe extern "C" {
    /// torajs-mmalloc libc-compat — v0.7-A2 step 6b cutover.
    #[link_name = "__torajs_malloc"]
    fn malloc(n: usize) -> *mut c_void;
    #[link_name = "__torajs_free"]
    fn free(p: *mut c_void, size: usize);

    /// Microtask queue (libtorajs_microtask.a). Pushed by
    /// `drain_callbacks` for each cb node when a Promise settles;
    /// pushed by `attach_then`'s fast path when source is already
    /// settled at attach time.
    fn __torajs_microtask_enqueue(fn_: MicrotaskFn, arg: i64);

    /// Throw substrate (libtorajs_throw.a) — sets the per-thread
    /// throw slot so the next emit_throw_check after `get_value`'s
    /// rejected path propagates the throw to the active try/catch.
    fn __torajs_throw_set(tag: i64, value: i64);

    /// Refcount kernel (libtorajs_rc.a) — the rejected path funds
    /// the throw slot's owned copy of a heap reason.
    fn __torajs_rc_inc(p: *mut c_void);
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
            free(node as *mut c_void, core::mem::size_of::<PromiseCb>());
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
        // P10.5-A3-b — every PENDING → REJECTED transition enqueues
        // an HPRT-check microtask. Synchronous .catch / .then(_,
        // onErr) / await attaches made same-tick set has_handler
        // first; the microtask defers the unhandled decision to the
        // natural drain order so those attaches are observed.
        crate::unhandled::enqueue_hprt_check(p);
    }
}

/// `await p` value extraction:
///   - FULFILLED → return value (raw i64 — heap ptrs returned as bits)
///   - REJECTED  → route through `__torajs_throw_set` + return 0;
///     emit_throw_check after the call sees the throw slot non-empty
///     and propagates.
///   - PENDING   → return 0 (sync-resolve model; the silent 0 guards
///     against crashes pre-event-loop).
///
/// P10.5-A3-a — `await` is `.then(resolveBinding, rejectBinding)`
/// per spec §14.5.4.1; the rejectBinding attaches a rejection handler.
/// Set `has_handler = 1` so the HPRT-check microtask sees the promise
/// as observed and skips the default reporter.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_get_value(p: *const c_void) -> i64 {
    if p.is_null() {
        return 0;
    }
    let pp = p as *mut Promise;
    unsafe { (*pp).has_handler = 1 };
    let state = unsafe { (*pp).state };
    if state == STATE_REJECTED {
        let tag = if unsafe { (*pp).value_is_heap } != 0 {
            THROW_TAG_ANY_HEAP
        } else {
            THROW_TAG_I64
        };
        // Rotation 326 — the throw slot's contract is OWNED (a
        // thrown `new Error` transfers its mint; the catch binding
        // releases it), but this cell keeps holding the reason and
        // releases it again at its own drop: handing the slot a
        // borrow charged one reference twice (`await p` on a
        // rejected promise underflowed the Error instance — the
        // string-reason shape only survived because static cells
        // no-op rc). Take the +1 here so both releases are funded;
        // re-awaiting the same rejected promise re-pays per throw.
        unsafe {
            if (*pp).value_is_heap != 0 && (*pp).value != 0 {
                __torajs_rc_inc((*pp).value as *mut c_void);
            }
            __torajs_throw_set(tag, (*pp).value);
        }
        return 0;
    }
    if state != STATE_FULFILLED {
        return 0;
    }
    unsafe { (*pp).value }
}

/// `await p` where the awaiting site knows which typed lane it will
/// read the result into (RFC 20260727 blade 3).
///
/// [`__torajs_promise_get_value`] answers the slot verbatim, which is
/// right whenever the cell's storage form already matches the lane.
/// It does not when the cell was settled from an `any`: the slot then
/// holds a NaN-box pointer, and `await`'s caller casts by the STATIC
/// inner type — bitcasting the pointer to f64 (NaN) or dereferencing
/// it as a Str. Same defect the `.then` kernels had, same fix: consult
/// the stamp, which is the only runtime record of the real form.
///
/// The mirror case is just as real: the awaiting site's lane is `any`
/// while the cell holds a typed form. `Promise.resolve(x)` on an `any`
/// argument answers the SAME cell when x is already a promise
/// (§27.2.4.7 step 2), so the result keeps whatever repr that promise
/// was minted with — an i64 1 under a static `Promise<any>`. The
/// caller then read the slot raw and rc_inc'd it as a NaN box, which
/// is `rc_inc(0x1)`. Asking for REPR_ANY boxes the slot per the cell's
/// own stamp instead.
///
/// `want_repr` of 0 means the caller has no lane in mind at all and
/// wants the slot as-is.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_get_value_as(p: *const c_void, want_repr: i64) -> i64 {
    let v = unsafe { __torajs_promise_get_value(p) };
    if p.is_null() || want_repr == 0 {
        return v;
    }
    let repr = unsafe { (*(p as *const Promise)).value_repr };
    if want_repr == REPR_ANY as i64 {
        // Rc-neutral like the unbox below — the caller's cast takes
        // the stake. A cell that already holds a box answers it back
        // verbatim.
        return unsafe { crate::then_box::box_settled(repr, v) };
    }
    if repr != REPR_ANY {
        return v;
    }
    unsafe { crate::then_box::unbox_settled(want_repr as u8, v) }
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
///
/// P10.5-A3-a — any `attach_then` (including `.then(onOk)` without
/// onRejected) marks the source as observed per spec §27.2.1.3
/// PerformPromiseThen step 12 (`SetPromiseIsHandled`). `.then(onOk)`
/// without onRejected still counts because the rejection is forwarded
/// into the result Promise via the dispatcher's REJECTED branch — the
/// chain has taken ownership of the source's rejection; if the result
/// itself is then unhandled, the result's own HPRT microtask reports
/// it.
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
    unsafe { (*pp).has_handler = 1 };
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

/// Stamp the cell's `value_repr` (RFC 20260720-anylane-promise-methods
/// knife 1). Emitted by the `box_to_any` family when a typed
/// `Promise<T>` crosses into the `any` world — the box site statically
/// knows T, the cell keeps the storage form for the any-lane
/// `.then`/`.catch` bridge. Idempotent: T is fixed per cell, so a
/// re-stamp writes the same code.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_stamp_repr(p: *mut c_void, repr: i64) {
    if p.is_null() {
        return;
    }
    unsafe { (*as_promise(p)).value_repr = repr as u8 };
}
