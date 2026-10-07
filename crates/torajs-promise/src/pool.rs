//! Promise free-list pool + 5 alloc variants + thenable absorption
//! + rc-aware drop.
//!
//! Port of the P-PERF.A6 pool + T-15.a alloc + T-19.f thenable
//! absorption + T-15.g.7 drop sections of `runtime_promise.c`
//! (P6.1, 2026-05-24). Hot Promise benchmarks (promise-chain-1m,
//! promise-all-1m, promise-await-100k, promise-then-100k) allocate
//! and free hundreds-of-thousands of Promises in tight loops — the
//! pool turns each pair into a head-only LIFO pop/push instead of
//! a libc malloc/free pair (~10× faster on hot paths).
//!
//! ## Pool layout (head-only stack, LIFO)
//!
//! `Promise.callbacks` doubles as the freelist `next` while parked
//! (an untagged link — a parked cell carries no reaction).
//! On alloc-out the pop clears it back to NULL; on release the
//! `callbacks` pointer is repurposed to point at the previous head.
//! No extra storage per slot. Capacity bounded at 32 — overflow
//! goes straight to `free()`.
//!
//! ## Thenable absorption (`Promise.resolve(p)` sync MVP)
//!
//! When `Promise.resolve(p)` is called and `p` is already a Promise,
//! return a fresh Promise with `p`'s state + value (per ES2015 spec:
//! `Promise.resolve(thenable)` returns `thenable` unchanged when it's
//! already a Promise). Sync MVP: PENDING inner → rejected outer
//! with placeholder reason (no callback fan-in yet; T-16 wires).
//!
//! Rc: outer takes one ref on inner's resolved value if heap; caller
//! still owns the original `p` ref.

use core::ffi::c_void;
use core::ptr;

use crate::layout::{
    HeapHeader, PROMISE_SIZE, Promise, PromiseCb, RX_NONE, STATE_FULFILLED, STATE_PENDING,
    STATE_REJECTED, TAG_PROMISE, as_promise,
};

mod teardown;
mod thenable;
pub use teardown::__torajs_promise_drop;
pub use thenable::__torajs_promise_resolve_thenable;

/// Bounded freelist capacity. Matches
/// `runtime_promise.c::__TORAJS_PROMISE_POOL_CAP = 32`.
const POOL_CAP: usize = 32;

/// `STATIC_LITERAL` flag bit — Promise literals don't exist in
/// practice but the drop bit-check mirrors the universal heap-header
/// flag scheme.
pub(crate) const FLAG_STATIC_LITERAL: u16 = 4;

/// `FLAG_SUBCLASSED` mirror (`torajs_rc::flags` bit 0, RFC 20260730
/// blade 0) — an exotic cell minted as a user-class instance; drop
/// must scrub its identity entry from torajs-meta's side table.
const FLAG_SUBCLASSED: u16 = 1;

use std::sync::atomic::{AtomicI32, AtomicPtr, Ordering};

/// Freelist head — `Promise.callbacks` repurposed as `next`.
/// AtomicPtr static pattern (same as torajs-arr::pool / torajs-weak::
/// registry); compiles to plain load/store on x86_64 / aarch64.
static POOL_HEAD: AtomicPtr<Promise> = AtomicPtr::new(ptr::null_mut());
// the count moves with the list head, which is a plain load and
// store; a read-modify-write here would buy no more than the head has
// and costs a locked add on every promise alloc and release
static POOL_COUNT: AtomicI32 = AtomicI32::new(0);

unsafe extern "C" {
    /// torajs-mmalloc libc-compat — v0.7-A2 step 6b cutover.
    #[link_name = "__torajs_malloc"]
    fn malloc(n: usize) -> *mut c_void;
    #[link_name = "__torajs_free"]
    fn free(p: *mut c_void, size: usize);
}

/// Internal alloc — pop pool or malloc, then init fields.
unsafe fn promise_alloc_(state: u8, value: i64, is_heap: u8) -> *mut Promise {
    let head = POOL_HEAD.load(Ordering::Relaxed);
    let p = if !head.is_null() {
        let next = unsafe { (*head).callbacks as *mut Promise };
        POOL_HEAD.store(next, Ordering::Relaxed);
        POOL_COUNT.store(POOL_COUNT.load(Ordering::Relaxed) - 1, Ordering::Relaxed);
        head
    } else {
        unsafe { malloc(PROMISE_SIZE) as *mut Promise }
    };
    unsafe {
        (*p).header = HeapHeader {
            refcount: 1,
            type_tag: TAG_PROMISE,
            flags: 0,
        };
        (*p).state = state;
        (*p).value_is_heap = is_heap;
        // P10.5-A3-a — fresh Promise starts with no handler attached.
        // attach_then / get_value set this to 1 as cb is hooked up;
        // unhandled::hprt_check_dispatch reads it.
        (*p).has_handler = 0;
        // Fresh cell has not crossed into the any world — the
        // box_to_any choke point stamps the real repr (RFC
        // 20260720-anylane-promise-methods knife 1).
        (*p).value_repr = crate::layout::REPR_UNSTAMPED;
        (*p).deferred_state = 0;
        (*p).rx_kind = RX_NONE;
        (*p).rx_ret_repr = 0;
        (*p).rx_param_repr = 0;
        (*p).value = value;
        (*p).callbacks = ptr::null_mut();
        // Lazy expando — drop cleared the previous tenant's before
        // release, but a fresh malloc block needs the NULL too.
        (*p).props = ptr::null_mut();
        (*p).rx_next = ptr::null_mut();
        (*p).rx_source = ptr::null_mut();
        (*p).rx_cb = ptr::null_mut();
    }
    p
}

/// Return a Promise to the pool if there's capacity, else free.
/// Caller must have already dropped any heap value + cb list.
#[inline(always)]
unsafe fn promise_release_(p: *mut Promise) {
    let count = POOL_COUNT.load(Ordering::Relaxed);
    if count < POOL_CAP as i32 {
        unsafe {
            (*p).callbacks = POOL_HEAD.load(Ordering::Relaxed) as *mut PromiseCb;
        }
        POOL_HEAD.store(p, Ordering::Relaxed);
        POOL_COUNT.store(count + 1, Ordering::Relaxed);
    } else {
        // Layer 1 sized free: Promise struct is PROMISE_SIZE — see
        // promise/layout.rs.
        unsafe { free(p as *mut c_void, crate::layout::PROMISE_SIZE) };
    }
}

// ============================================================
// Public (C-callable) alloc API. 5 variants — primitive vs heap-
// value × pending/fulfilled/rejected.
// ============================================================

/// # Safety
/// Takes no pointer arguments.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_alloc_pending() -> *mut c_void {
    unsafe { promise_alloc_(STATE_PENDING, 0, 0) as *mut c_void }
}

/// # Safety
/// `value` is a primitive payload; it is stored as raw bits and never dereferenced.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_alloc_fulfilled(value: i64) -> *mut c_void {
    unsafe { promise_alloc_(STATE_FULFILLED, value, 0) as *mut c_void }
}

/// # Safety
/// `reason` is a primitive payload; it is stored as raw bits and never dereferenced.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_alloc_rejected(reason: i64) -> *mut c_void {
    let p = unsafe { promise_alloc_(STATE_REJECTED, reason, 0) as *mut c_void };
    // P10.5-A3-b — REJECTED at construction enqueues an HPRT-check
    // microtask. See `unhandled::enqueue_hprt_check` for the
    // detection invariant; defers the unhandled decision until the
    // microtask drain so synchronous `.catch` attaches can mark
    // the promise as observed first.
    unsafe { crate::unhandled::enqueue_hprt_check(p) };
    p
}

/// Heap-value variant — caller transfers ONE refcount on `value` to
/// the Promise; drop dec's that ref via `__torajs_value_drop_heap`
/// when the Promise itself reaches rc=0.
///
/// # Safety
/// `value` is a live heap cell and the caller hands one reference to the Promise.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_alloc_fulfilled_heap(value: i64) -> *mut c_void {
    unsafe { promise_alloc_(STATE_FULFILLED, value, 1) as *mut c_void }
}

/// # Safety
/// `reason` is a live heap cell and the caller hands one reference to the Promise.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_alloc_rejected_heap(reason: i64) -> *mut c_void {
    let p = unsafe { promise_alloc_(STATE_REJECTED, reason, 1) as *mut c_void };
    // P10.5-A3-b — symmetric with `alloc_rejected`. The heap-typed
    // reason (Str / Obj / Any-box / etc.) is reachable through
    // `(*pp).value` so `unhandled::fire_unhandled_reporter` can
    // dispatch on its type_tag.
    unsafe { crate::unhandled::enqueue_hprt_check(p) };
    p
}

/// §27.2.4.7 step 2 through the ANY lane — `Promise.resolve(x)`
/// where `x`'s static type is Any. When the boxed value IS a
/// %Promise% cell, answer the same cell (`Promise.resolve(p) === p`;
/// tr has no Promise subclassing): the caller transferred one ref on
/// the box — for a heap tag that IS one ref on the cell — and the
/// return carries it through. Everything else takes exactly the
/// `alloc_fulfilled_heap` + `REPR_ANY` stamp pair the lowering used
/// to emit, folded here so the pass-through branch cannot be stamped
/// over (stamping an adopted cell would overwrite the inner
/// promise's own repr).
///
/// # Safety
/// `bits` is a NaN-boxed value whose heap payload, if any, is live; the caller hands its reference over.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_resolve_any(bits: i64) -> *mut c_void {
    unsafe {
        // Cell-likeness first (rotation 546 form): a ShortStr reports
        // tag Heap and `unbox_value` would leak one materialized Str
        // per probe. A cell's box IS the raw pointer, so no unbox is
        // needed at all — and a cell-like pattern is Heap-tagged by
        // construction, so the tag probe folds away too.
        if crate::unhandled::reason_is_cell_like(bits)
            && (*(bits as *const crate::layout::HeapHeader)).type_tag == TAG_PROMISE
        {
            return bits as *mut c_void;
        }
        let p = __torajs_promise_alloc_fulfilled_heap(bits);
        (*as_promise(p)).value_repr = crate::layout::REPR_ANY;
        p
    }
}

/// §27.2.4.6 through the ANY lane — a fresh REJECTED promise whose
/// reason is the boxed value verbatim (step 3 rejects WITH the
/// value; no thenable absorption, a promise reason included). The
/// caller transfers one ref on the box — `resolve_any`'s ledger.
///
/// # Safety
/// `bits` is a NaN-boxed value whose heap payload, if any, is live; the caller hands its reference over.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_reject_any(bits: i64) -> *mut c_void {
    unsafe {
        let p = __torajs_promise_alloc_rejected_heap(bits);
        (*as_promise(p)).value_repr = crate::layout::REPR_ANY;
        p
    }
}
