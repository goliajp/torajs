//! Promise cells: the 5 alloc variants, thenable absorption and the
//! rc-aware drop.
//!
//! Hot Promise benchmarks (promise-chain-1m, promise-all-1m,
//! promise-await-100k, promise-then-100k) allocate and free hundreds
//! of thousands of cells in tight loops. A cell is one block of the
//! allocator's row for its class, popped and pushed inline
//! (`torajs_mmalloc::fixed`): no call, no size checks, and no second
//! cache in front of the row.
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
    HeapHeader, PROMISE_SIZE, Promise, REPR_UNSTAMPED, RX_NONE, STATE_FULFILLED, STATE_PENDING,
    STATE_REJECTED, TAG_PROMISE, as_promise,
};

mod teardown;
mod thenable;
pub use teardown::__torajs_promise_drop;
pub use thenable::__torajs_promise_resolve_thenable;

/// `STATIC_LITERAL` flag bit — Promise literals don't exist in
/// practice but the drop bit-check mirrors the universal heap-header
/// flag scheme.
pub(crate) const FLAG_STATIC_LITERAL: u16 = 4;

/// `FLAG_SUBCLASSED` mirror (`torajs_rc::flags` bit 0, RFC 20260730
/// blade 0) — an exotic cell minted as a user-class instance; drop
/// must scrub its identity entry from torajs-meta's side table.
const FLAG_SUBCLASSED: u16 = 1;

use torajs_mmalloc::fixed::{alloc_fixed, free_fixed, pop_fixed};

unsafe extern "C" {
    /// torajs-mmalloc libc-compat — v0.7-A2 step 6b cutover.
    #[link_name = "__torajs_free"]
    fn free(p: *mut c_void, size: usize);
}

/// Internal alloc — a block of the cell's class, then init fields.
/// Inlined into runtime callers that already keep a frame, where the
/// allocator call on an empty row costs no more than the pop around
/// it.
#[inline(always)]
unsafe fn promise_alloc_(state: u8, value: i64, is_heap: u8) -> *mut Promise {
    let p = alloc_fixed::<PROMISE_SIZE>() as *mut Promise;
    unsafe { init_cell(p, state, value, is_heap, REPR_UNSTAMPED) };
    p
}

/// The leaf C entry points' alloc: the row pop needs no frame, and an
/// empty row tail-calls an allocating path instantiated per state, so
/// it keeps only the registers its own entry point needs.
#[inline(always)]
unsafe fn promise_alloc_leaf<const STATE: u8, const IS_HEAP: u8>(
    value: i64,
    repr: u8,
) -> *mut c_void {
    let p = pop_fixed::<PROMISE_SIZE>() as *mut Promise;
    if p.is_null() {
        return unsafe { promise_alloc_fresh::<STATE, IS_HEAP>(value, repr) };
    }
    unsafe { init_cell(p, STATE, value, IS_HEAP, repr) };
    p as *mut c_void
}

#[inline(never)]
unsafe fn promise_alloc_fresh<const STATE: u8, const IS_HEAP: u8>(
    value: i64,
    repr: u8,
) -> *mut c_void {
    let p = alloc_fixed::<PROMISE_SIZE>() as *mut Promise;
    unsafe { init_cell(p, STATE, value, IS_HEAP, repr) };
    p as *mut c_void
}

/// In-runtime twins of the leaf C entry points below, for callers that
/// already keep a frame.
#[inline(always)]
pub(crate) unsafe fn alloc_pending() -> *mut c_void {
    unsafe { promise_alloc_(STATE_PENDING, 0, 0) as *mut c_void }
}

/// [`alloc_pending`] off the row alone: null when the row is empty,
/// so a caller that keeps no frame can leave the allocator call to
/// its slow path.
#[inline(always)]
pub(crate) unsafe fn pop_pending() -> *mut c_void {
    let p = pop_fixed::<PROMISE_SIZE>() as *mut Promise;
    if !p.is_null() {
        unsafe { init_cell(p, STATE_PENDING, 0, 0, REPR_UNSTAMPED) };
    }
    p as *mut c_void
}

#[inline(always)]
pub(crate) unsafe fn alloc_fulfilled(value: i64) -> *mut c_void {
    unsafe { promise_alloc_(STATE_FULFILLED, value, 0) as *mut c_void }
}

#[inline(always)]
pub(crate) unsafe fn alloc_fulfilled_heap(value: i64) -> *mut c_void {
    unsafe { promise_alloc_(STATE_FULFILLED, value, 1) as *mut c_void }
}

#[inline(always)]
unsafe fn init_cell(p: *mut Promise, state: u8, value: i64, is_heap: u8, repr: u8) {
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
        // UNSTAMPED unless the mint site knows the value's form — the
        // box_to_any choke point stamps the real repr (RFC
        // 20260720-anylane-promise-methods knife 1).
        (*p).value_repr = repr;
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
}

/// Give a dead cell's block back to the allocator. Caller must have
/// already dropped any heap value + cb list.
#[inline(always)]
unsafe fn promise_release_(p: *mut Promise) {
    unsafe { free_fixed::<PROMISE_SIZE>(p as *mut u8) };
}

// ============================================================
// Public (C-callable) alloc API. 5 variants — primitive vs heap-
// value × pending/fulfilled/rejected.
// ============================================================

/// # Safety
/// Takes no pointer arguments.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_alloc_pending() -> *mut c_void {
    unsafe { promise_alloc_leaf::<STATE_PENDING, 0>(0, REPR_UNSTAMPED) }
}

/// # Safety
/// `value` is a primitive payload; it is stored as raw bits and never dereferenced.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_alloc_fulfilled(value: i64) -> *mut c_void {
    unsafe { promise_alloc_leaf::<STATE_FULFILLED, 0>(value, REPR_UNSTAMPED) }
}

/// [`__torajs_promise_alloc_fulfilled`] with the value's repr code
/// stamped (`__torajs_promise_stamp_repr`) — the pair a typed
/// `Promise.resolve(v)` mints, in one call.
///
/// # Safety
/// `value` is a primitive payload; it is stored as raw bits and never dereferenced.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_alloc_fulfilled_repr(
    value: i64,
    repr: i64,
) -> *mut c_void {
    unsafe { promise_alloc_leaf::<STATE_FULFILLED, 0>(value, repr as u8) }
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
    unsafe { promise_alloc_leaf::<STATE_FULFILLED, 1>(value, REPR_UNSTAMPED) }
}

/// [`__torajs_promise_alloc_fulfilled_heap`] with the value's repr code
/// stamped, as [`__torajs_promise_alloc_fulfilled_repr`].
///
/// # Safety
/// `value` is a live heap cell and the caller hands one reference to the Promise.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_alloc_fulfilled_heap_repr(
    value: i64,
    repr: i64,
) -> *mut c_void {
    unsafe { promise_alloc_leaf::<STATE_FULFILLED, 1>(value, repr as u8) }
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
        let p = alloc_fulfilled_heap(bits);
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
