//! `Promise.resolve(p)` thenable absorption, moved out of `pool.rs`.

use core::ffi::c_void;

use super::{
    __torajs_promise_alloc_fulfilled, __torajs_promise_alloc_fulfilled_heap,
    __torajs_promise_alloc_rejected, __torajs_promise_alloc_rejected_heap,
};
use crate::layout::{STATE_FULFILLED, STATE_REJECTED, as_promise};

unsafe extern "C" {
    /// torajs-rc. Bumps refcount by 1 (NaN-box gated; the kernel's
    /// own stakes on known cells use `torajs_rc::rc_inc_cell`).
    fn __torajs_rc_inc(p: *mut c_void);
}

/// `Promise.resolve(p)` thenable absorption. When `p` is itself a
/// Promise, return a fresh Promise with the same state + value
/// (per ES2015 spec). PENDING inner → rejected outer with placeholder
/// reason (MVP; T-16 wires real callback fan-in).
///
/// # Safety
/// `p` is NULL or a live Promise cell.
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
