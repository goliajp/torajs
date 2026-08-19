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

/// `FLAG_SUBCLASSED` mirror (`torajs_rc::flags` bit 0, RFC 20260730
/// blade 0) — an exotic cell minted as a user-class instance; drop
/// must scrub its identity entry from torajs-meta's side table.
const FLAG_SUBCLASSED: u16 = 1;

use std::sync::atomic::{AtomicI32, AtomicPtr, Ordering};

/// Freelist head — `Promise.callbacks` repurposed as `next`.
/// AtomicPtr static pattern (same as torajs-arr::pool / torajs-weak::
/// registry); compiles to plain load/store on x86_64 / aarch64.
static POOL_HEAD: AtomicPtr<Promise> = AtomicPtr::new(ptr::null_mut());
static POOL_COUNT: AtomicI32 = AtomicI32::new(0);

unsafe extern "C" {
    /// torajs-mmalloc libc-compat — v0.7-A2 step 6b cutover.
    #[link_name = "__torajs_malloc"]
    fn malloc(n: usize) -> *mut c_void;
    #[link_name = "__torajs_free"]
    fn free(p: *mut c_void, size: usize);

    /// Provided by torajs-rc (libtorajs_rc.a) at `tr build` link.
    /// Returns 0 if rc stays positive, 1 on transition to zero.
    fn __torajs_rc_dec(p: *mut c_void) -> i32;
    /// torajs-rc. Bumps refcount by 1.
    fn __torajs_rc_inc(p: *mut c_void);
    /// Universal-drop dispatcher in runtime_str.c. Routes the heap
    /// pointer to its type-specific drop helper.
    fn __torajs_value_drop_heap(p: *mut c_void);
    /// torajs-meta — scrub a dying exotic-subclass instance's
    /// identity entry (RFC 20260730 blade 0); gated on
    /// `FLAG_SUBCLASSED` so plain promises never call out.
    fn __torajs_subclass_drop_entry(p: *mut c_void);
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
        // P10.5-A3-a — fresh Promise starts with no handler attached.
        // attach_then / get_value set this to 1 as cb is hooked up;
        // unhandled::hprt_check_dispatch reads it.
        (*p).has_handler = 0;
        // Fresh cell has not crossed into the any world — the
        // box_to_any choke point stamps the real repr (RFC
        // 20260720-anylane-promise-methods knife 1).
        (*p).value_repr = crate::layout::REPR_UNSTAMPED;
        // Zero `_pad` so memcmp on the whole struct is well-defined.
        (*p)._pad = [0; 4];
        (*p).value = value;
        (*p).callbacks = ptr::null_mut();
        // Lazy expando — drop cleared the previous tenant's before
        // release, but a fresh malloc block needs the NULL too.
        (*p).props = ptr::null_mut();
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
        // Layer 1 sized free: Promise struct is PROMISE_SIZE (40 B)
        // — see promise/layout.rs (Step 4d).
        unsafe { free(p as *mut c_void, crate::layout::PROMISE_SIZE) };
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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_alloc_fulfilled_heap(value: i64) -> *mut c_void {
    unsafe { promise_alloc_(STATE_FULFILLED, value, 1) as *mut c_void }
}

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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_resolve_any(bits: i64) -> *mut c_void {
    unsafe extern "C" {
        fn __torajs_anyv_unbox_tag(v: u64) -> i64;
        fn __torajs_anyv_unbox_value(v: u64) -> i64;
    }
    // AnySlotTag heap index (`nanbox_encode` reports 4 for the
    // heap-cell tag — the `then_box::box_settled` REPR_HEAP mirror).
    const TAG_HEAP: i64 = 4;
    unsafe {
        if __torajs_anyv_unbox_tag(bits as u64) == TAG_HEAP {
            let raw = __torajs_anyv_unbox_value(bits as u64);
            if crate::unhandled::reason_is_cell_like(raw)
                && (*(raw as *const crate::layout::HeapHeader)).type_tag == TAG_PROMISE
            {
                return raw as *mut c_void;
            }
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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_reject_any(bits: i64) -> *mut c_void {
    unsafe {
        let p = __torajs_promise_alloc_rejected_heap(bits);
        (*as_promise(p)).value_repr = crate::layout::REPR_ANY;
        p
    }
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
    // The fresh cell shares the source's value verbatim, so its
    // storage form travels along (RFC 20260720-anylane-promise-methods
    // knife 1; UNSTAMPED source → UNSTAMPED result, still loud).
    let value_repr = unsafe { (*pp).value_repr };
    if state == STATE_FULFILLED {
        let out = if value_is_heap != 0 && value != 0 {
            unsafe { __torajs_rc_inc(value as *mut c_void) };
            unsafe { __torajs_promise_alloc_fulfilled_heap(value) }
        } else {
            unsafe { __torajs_promise_alloc_fulfilled(value) }
        };
        unsafe { (*as_promise(out)).value_repr = value_repr };
        return out;
    }
    if state == STATE_REJECTED {
        let out = if value_is_heap != 0 && value != 0 {
            unsafe { __torajs_rc_inc(value as *mut c_void) };
            unsafe { __torajs_promise_alloc_rejected_heap(value) }
        } else {
            unsafe { __torajs_promise_alloc_rejected(value) }
        };
        unsafe { (*as_promise(out)).value_repr = value_repr };
        return out;
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
            free(node as *mut c_void, core::mem::size_of::<PromiseCb>());
            node = next;
        }
        (*pp).callbacks = ptr::null_mut();
        // Heap-typed resolved value gets a type-specific drop.
        if (*pp).value_is_heap != 0 && (*pp).state != STATE_PENDING && (*pp).value != 0 {
            __torajs_value_drop_heap((*pp).value as *mut c_void);
        }
        if (*pp).header.flags & FLAG_SUBCLASSED != 0 {
            __torajs_subclass_drop_entry(p);
        }
        // Expando props dynobj (defineProperty landings) — the
        // universal dispatcher routes it to the dynobj drop; NULLed
        // so a pool re-issue starts clean.
        if !(*pp).props.is_null() {
            __torajs_value_drop_heap((*pp).props);
            (*pp).props = ptr::null_mut();
        }
        promise_release_(pp);
    }
}
