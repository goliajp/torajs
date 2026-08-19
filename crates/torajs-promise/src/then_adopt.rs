//! Thenable adoption for the typed `.then` / `.catch` kernels —
//! §27.2.1.3.2 NewPromiseResolveThenableJob.
//!
//! When a reaction handler returns a promise, the derived promise does
//! NOT fulfill with that promise as its value: it ADOPTS it, settling
//! with whatever the inner one eventually settles with. Without this a
//! `.then(cb)` chain hands the next handler a promise object where the
//! spec (and every engine) hands it the unwrapped value, and the
//! checker's `Promise<Promise<U>>` result type is the visible symptom
//! of the same gap.
//!
//! The sibling [`crate::pool::__torajs_promise_resolve_thenable`]
//! absorbs a thenable by SNAPSHOT — it copies a settled inner cell's
//! state and rejects outright when the inner is still PENDING, which is
//! precisely the common case here (a handler returning an async call's
//! promise). Adoption instead ATTACHES to the inner cell, so a pending
//! inner simply settles the outer later, through the same microtask
//! ordering the engines produce.

use core::ffi::c_void;

use crate::layout::{
    HeapHeader, Promise, REPR_ANY, REPR_HEAP, STATE_REJECTED, TAG_PROMISE, as_promise,
};
use crate::pool::__torajs_promise_drop;
use crate::state::{
    __torajs_promise_attach_then, __torajs_promise_reject, __torajs_promise_resolve,
};
use crate::unhandled::reason_is_cell_like;

unsafe extern "C" {
    #[link_name = "__torajs_libc_malloc"]
    fn malloc(n: usize) -> *mut c_void;
    #[link_name = "__torajs_libc_free"]
    fn free(p: *mut c_void);
    fn __torajs_rc_inc(p: *mut c_void);
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    fn __torajs_throw_take() -> i64;
    fn __torajs_throw_take_tag() -> i64;
}

/// Is this handler return a promise cell the derived promise must
/// adopt? Only the two reprs that can carry one are probed — a
/// `REPR_HEAP` return moves a raw cell pointer, a `REPR_ANY` one a
/// NaN-box whose heap tag has to be peeled first. Every other repr
/// names a form no promise fits in.
///
/// tr has no user-defined thenables (no `Symbol.species`, no
/// subclassing), so "is a %Promise% cell" is the whole of §27.2.1.3.2
/// step 8's `IsCallable(then)` probe here.
pub(crate) unsafe fn returned_promise(repr: u8, value: i64) -> Option<*mut c_void> {
    unsafe {
        let raw = match repr {
            REPR_HEAP => value,
            REPR_ANY => {
                // NaN-box gate, mirroring `combinator_any::slot_promise`:
                // immediates and the non-heap tag bit are not cells.
                const TOP_16_MASK: u64 = 0xFFFF_0000_0000_0000;
                const TAG_BIT_TYPE_OTHER: u64 = 0x02;
                let bits = value as u64;
                if bits & TOP_16_MASK != 0 || bits & TAG_BIT_TYPE_OTHER != 0 {
                    return None;
                }
                value
            }
            _ => return None,
        };
        if !reason_is_cell_like(raw) {
            return None;
        }
        if (*(raw as *const HeapHeader)).type_tag != TAG_PROMISE {
            return None;
        }
        Some(raw as *mut c_void)
    }
}

#[repr(C)]
struct AdoptArg {
    inner: *mut c_void,
    result: *mut c_void,
}

/// Copy the inner cell's settlement onto the outer one. The value slot
/// is shared, so a droppable stake gains one owner — the same transfer
/// `pool::__torajs_promise_resolve_thenable` performs for its snapshot
/// form.
unsafe extern "C" fn adopt_dispatch(arg: i64) {
    let a = arg as *mut AdoptArg;
    unsafe {
        let ip: *mut Promise = as_promise((*a).inner);
        let rp: *mut Promise = as_promise((*a).result);
        let value = (*ip).value;
        let is_heap = (*ip).value_is_heap;
        (*rp).value_is_heap = is_heap;
        (*rp).value_repr = (*ip).value_repr;
        if is_heap != 0 && value != 0 {
            __torajs_rc_inc(value as *mut c_void);
        }
        if (*ip).state == STATE_REJECTED {
            __torajs_promise_reject((*a).result, value);
        } else {
            __torajs_promise_resolve((*a).result, value);
        }
        __torajs_promise_drop((*a).inner);
        __torajs_promise_drop((*a).result);
        free(a as *mut c_void);
    }
}

/// Settle `result` from `inner` whenever `inner` settles.
///
/// Ownership: the handler returned `inner` owned and nobody else takes
/// it, so that stake TRANSFERS into the job's arg block and the
/// dispatcher releases it — no inc here. `result` needs one of its own,
/// since the reaction dispatcher that called us drops its stake as soon
/// as it returns.
pub(crate) unsafe fn adopt_into(result: *mut c_void, inner: *mut c_void) {
    unsafe {
        // §27.2.1.3.2 step 1 — resolving a promise with itself is a
        // TypeError, not an eternal pending. Reject rather than build a
        // job whose completion depends on the promise it would settle.
        if core::ptr::eq(result, inner) {
            __torajs_throw_type_error(c"Chaining cycle detected for promise".as_ptr());
            let tag = __torajs_throw_take_tag();
            let value = __torajs_throw_take();
            let reason = __torajs_anyv_box_from_pair(tag, value);
            let rp = as_promise(result);
            (*rp).value_is_heap = 1;
            (*rp).value_repr = REPR_ANY;
            __torajs_promise_reject(result, reason as i64);
            __torajs_promise_drop(inner);
            return;
        }
        let a = malloc(core::mem::size_of::<AdoptArg>()) as *mut AdoptArg;
        (*a).inner = inner;
        (*a).result = result;
        __torajs_rc_inc(result);
        __torajs_promise_attach_then(inner, Some(adopt_dispatch), a as i64);
    }
}

/// The same protocol for the any-lane bridge, which lives in
/// torajs-anyvalue (`method_call_promise.rs::then_any_dispatch`) and so
/// reaches adoption across the C boundary. Answers non-zero when it
/// adopted — the caller then skips its own resolve leg.
///
/// A heap-tagged NaN box IS the cell pointer, so the handler's returned
/// stake on the box is the stake on the promise: it transfers into the
/// job block exactly as in [`adopt_into`], and the caller must not
/// release it on this path.
///
/// # Safety
///
/// `result` is a live PENDING promise cell; `value` carries the form
/// `repr` names.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_adopt_if_thenable(
    result: *mut c_void,
    repr: i64,
    value: i64,
) -> i64 {
    unsafe {
        let Some(inner) = returned_promise(repr as u8, value) else {
            return 0;
        };
        adopt_into(result, inner);
        1
    }
}

/// §27.2.5.4 with both handlers absent — `p.then()` / `p.catch()`:
/// onFulfilled falls back to Identity and onRejected to Thrower, so
/// the derived promise settles exactly as the source does, one
/// reaction tick later — which is this module's adoption contract.
/// The derived cell mints pending (its repr is overwritten by
/// [`adopt_dispatch`]'s inner-to-outer copy when the source
/// settles), the source takes the has-handler mark (its rejection
/// is handled — the derived promise is the live complaint), and the
/// job's source stake is a fresh inc since the caller's box is a
/// borrow.
///
/// # Safety
/// `src` is a live promise cell the caller keeps alive across the
/// call. The answer is an owned pending cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_then_passthrough(src: *mut c_void) -> *mut c_void {
    unsafe {
        let d = crate::pool::__torajs_promise_alloc_pending();
        let sp = as_promise(src);
        let dp = as_promise(d);
        // The derived settles with the source's own value form, so
        // its repr stamp is the source's — a downstream any-param
        // attach reads it while the derived is still pending
        // (`refuse_unstamped`), and the adopt job re-copies it at
        // settle time anyway.
        (*dp).value_repr = (*sp).value_repr;
        (*dp).value_is_heap = (*sp).value_is_heap;
        (*sp).has_handler = 1;
        __torajs_rc_inc(src);
        adopt_into(d, src);
        d
    }
}
