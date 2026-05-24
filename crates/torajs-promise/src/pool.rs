//! Promise free-list pool + 5 alloc variants + thenable absorption
//! + rc-aware drop.
//!
//! Port of the P-PERF.A6 pool + T-15.a alloc + T-19.f thenable
//! absorption + T-15.g.7 drop sections of `runtime_promise.c`
//! (P6.1, 2026-05-24). Hot Promise benchmarks (promise-chain-1k,
//! promise-all-1k, promise-await-100k, promise-then-100k) allocate
//! + free hundreds-of-thousands of Promises in tight loops — the
//! pool turns each pair into a head-only LIFO pop/push instead of
//! a libc malloc/free pair (~10× faster on hot paths).
//!
//! ## Pool layout (head-only stack, LIFO)
//!
//! `Promise.callbacks` doubles as the freelist `next` while parked.
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
    HeapHeader, PROMISE_SIZE, Promise, PromiseCb, STATE_FULFILLED, STATE_PENDING, STATE_REJECTED,
    TAG_PROMISE, as_promise,
};

/// Bounded freelist capacity. Matches
/// `runtime_promise.c::__TORAJS_PROMISE_POOL_CAP = 32`.
const POOL_CAP: usize = 32;

/// `STATIC_LITERAL` flag bit — Promise literals don't exist in
/// practice but the drop bit-check mirrors the universal heap-header
/// flag scheme.
const FLAG_STATIC_LITERAL: u16 = 4;

use std::sync::atomic::{AtomicI32, AtomicPtr, Ordering};

/// Freelist head — `Promise.callbacks` repurposed as `next`.
/// AtomicPtr static pattern (same as torajs-arr::pool / torajs-weak::
/// registry); compiles to plain load/store on x86_64 / aarch64.
static POOL_HEAD: AtomicPtr<Promise> = AtomicPtr::new(ptr::null_mut());
static POOL_COUNT: AtomicI32 = AtomicI32::new(0);

unsafe extern "C" {
    fn malloc(n: usize) -> *mut c_void;
    fn free(p: *mut c_void);

    /// Provided by torajs-rc (libtorajs_rc.a) at `tr build` link.
    /// Returns 0 if rc stays positive, 1 on transition to zero.
    fn __torajs_rc_dec(p: *mut c_void) -> i32;
    /// torajs-rc. Bumps refcount by 1.
    fn __torajs_rc_inc(p: *mut c_void);
    /// Universal-drop dispatcher in runtime_str.c. Routes the heap
    /// pointer to its type-specific drop helper.
    fn __torajs_value_drop_heap(p: *mut c_void);
}

/// Internal alloc — pop pool or malloc, then init fields.
unsafe fn promise_alloc_(state: u8, value: i64, is_heap: u8) -> *mut Promise {
    let head = POOL_HEAD.load(Ordering::Relaxed);
    let p = if !head.is_null() {
        let next = unsafe { (*head).callbacks as *mut Promise };
        POOL_HEAD.store(next, Ordering::Relaxed);
        POOL_COUNT.fetch_sub(1, Ordering::Relaxed);
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
        // Zero `_pad` so memcmp on the whole struct is well-defined.
        (*p)._pad = [0; 6];
        (*p).value = value;
        (*p).callbacks = ptr::null_mut();
    }
    p
}

/// Return a Promise to the pool if there's capacity, else free.
/// Caller must have already dropped any heap value + cb list.
unsafe fn promise_release_(p: *mut Promise) {
    let count = POOL_COUNT.load(Ordering::Relaxed);
    if count < POOL_CAP as i32 {
        unsafe {
            (*p).callbacks = POOL_HEAD.load(Ordering::Relaxed) as *mut PromiseCb;
        }
        POOL_HEAD.store(p, Ordering::Relaxed);
        POOL_COUNT.fetch_add(1, Ordering::Relaxed);
    } else {
        unsafe { free(p as *mut c_void) };
    }
}

// ============================================================
// Public (C-callable) alloc API. 5 variants — primitive vs heap-
// value × pending/fulfilled/rejected.
// ============================================================

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_alloc_pending() -> *mut c_void {
    unsafe { promise_alloc_(STATE_PENDING, 0, 0) as *mut c_void }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_alloc_fulfilled(value: i64) -> *mut c_void {
    unsafe { promise_alloc_(STATE_FULFILLED, value, 0) as *mut c_void }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_alloc_rejected(reason: i64) -> *mut c_void {
    unsafe { promise_alloc_(STATE_REJECTED, reason, 0) as *mut c_void }
}

/// Heap-value variant — caller transfers ONE refcount on `value` to
/// the Promise; drop dec's that ref via `__torajs_value_drop_heap`
/// when the Promise itself reaches rc=0.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_alloc_fulfilled_heap(value: i64) -> *mut c_void {
    unsafe { promise_alloc_(STATE_FULFILLED, value, 1) as *mut c_void }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_alloc_rejected_heap(reason: i64) -> *mut c_void {
    unsafe { promise_alloc_(STATE_REJECTED, reason, 1) as *mut c_void }
}

/// `Promise.resolve(p)` thenable absorption. When `p` is itself a
/// Promise, return a fresh Promise with the same state + value
/// (per ES2015 spec). PENDING inner → rejected outer with placeholder
/// reason (MVP; T-16 wires real callback fan-in).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_resolve_thenable(p: *mut c_void) -> *mut c_void {
    if p.is_null() {
        return unsafe { __torajs_promise_alloc_fulfilled(0) };
    }
    let pp = as_promise(p);
    let state = unsafe { (*pp).state };
    let value_is_heap = unsafe { (*pp).value_is_heap };
    let value = unsafe { (*pp).value };
    if state == STATE_FULFILLED {
        if value_is_heap != 0 && value != 0 {
            unsafe { __torajs_rc_inc(value as *mut c_void) };
            return unsafe { __torajs_promise_alloc_fulfilled_heap(value) };
        }
        return unsafe { __torajs_promise_alloc_fulfilled(value) };
    }
    if state == STATE_REJECTED {
        if value_is_heap != 0 && value != 0 {
            unsafe { __torajs_rc_inc(value as *mut c_void) };
            return unsafe { __torajs_promise_alloc_rejected_heap(value) };
        }
        return unsafe { __torajs_promise_alloc_rejected(value) };
    }
    // PENDING inner — sync MVP can't represent suspension. Reject
    // with placeholder reason so user sees clear test failure rather
    // than a silent wrong-value pass through.
    unsafe { __torajs_promise_alloc_rejected(0) }
}

/// rc-aware drop. Called from `value_drop_heap`'s TAG_PROMISE case.
/// On last owner: free residual cb list, drop heap value if any,
/// return Promise to pool (or free if pool full).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_drop(p: *mut c_void) {
    if p.is_null() {
        return;
    }
    if unsafe { __torajs_rc_dec(p) } == 0 {
        return;
    }
    let pp = as_promise(p);
    unsafe {
        // Defensive: STATIC_LITERAL Promise (none in practice today;
        // the universal drop dispatch sees the bit and skips, but
        // re-check here to keep the invariant if dispatch path
        // changes).
        if (*pp).header.flags & FLAG_STATIC_LITERAL != 0 {
            return;
        }
        // Free any unfired callback list nodes.
        let mut node = (*pp).callbacks;
        while !node.is_null() {
            let next = (*node).next;
            free(node as *mut c_void);
            node = next;
        }
        (*pp).callbacks = ptr::null_mut();
        // Heap-typed resolved value gets a type-specific drop.
        if (*pp).value_is_heap != 0 && (*pp).state != STATE_PENDING && (*pp).value != 0 {
            __torajs_value_drop_heap((*pp).value as *mut c_void);
        }
        promise_release_(pp);
    }
}
