//! `Promise.all<T>(Promise<T>[]) → Promise<T[]>`, the sync kernel over
//! a promises array. Sibling of [`crate::combinator`], which keeps
//! `race` / `any` and the slot accessors every kernel shares.

use core::ffi::c_void;

use crate::combinator::{
    absorb_inputs, arr_len, arr_slot_ptr, defer_settle, settle_result, sparse_input_rejects,
};
use crate::layout::{
    REPR_ANY, REPR_BOOL, REPR_F64, REPR_HEAP, REPR_I64, REPR_STR, REPR_UNSTAMPED, REPR_VOID,
    STATE_FULFILLED, STATE_REJECTED,
};

unsafe extern "C" {
    fn __torajs_rc_inc(p: *mut c_void);
    /// Array<i64> alloc + push, defined in libtorajs_arr.a — how
    /// Promise.all builds its result Array.
    fn __torajs_arr_alloc(initial_cap: u64) -> *mut c_void;
    fn __torajs_arr_push(arr: *mut c_void, val: i64) -> *mut c_void;
    /// torajs-arr — elem-kind self-description (RFC 20260704 S1);
    /// the result array is any-consumable via the settled cell's
    /// REPR_HEAP stamp, so it needs the mark too.
    fn __torajs_arr_mark_kind(arr: *mut c_void, chain: u64);
}

/// Map a source promise's value repr onto the result array's
/// elem-kind chain (torajs-rc `arr_kind` numbering: 1=I64, 2=F64,
/// 3=BOOL, 4=heap-cell). `None` = a form the raw-slot array cannot
/// self-describe (AnyValue bits / void / null fillers) — the caller
/// leaves the settled cell UNSTAMPED so the any lane stays loud
/// instead of misdecoding slots.
pub(crate) fn repr_arr_kind_chain(repr: u8) -> Option<u64> {
    match repr {
        REPR_I64 => Some(1),
        REPR_F64 => Some(2),
        REPR_BOOL => Some(3),
        REPR_STR | REPR_HEAP => Some(4),
        _ => None,
    }
}

/// # Safety
/// `promises_arr` is NULL or a live array whose elements are live Promise cells or values of the declared element lane.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_all_sync(
    promises_arr: *mut c_void,
    target_repr: i64,
) -> *mut c_void {
    let target_repr = target_repr as u8;
    if promises_arr.is_null() {
        return unsafe { defer_settle(STATE_REJECTED, 0, 0, REPR_VOID) };
    }
    if unsafe { sparse_input_rejects(promises_arr) } {
        return unsafe { defer_settle(STATE_REJECTED, 0, 0, REPR_VOID) };
    }
    // An element wearing its own `then` sends the input down the
    // interleaved lane (§27.2.4.1.3 step 6.q-s); see
    // `combinator_then_override`.
    if let Some(out) = unsafe {
        crate::combinator_then_override::defer_on_then_override(
            promises_arr,
            crate::combinator_all_fanin::MODE_ALL,
            crate::combinator_dyn::all_sync_untargeted,
        )
    } {
        return out;
    }
    // §27.2.4 static-slot patch consult (rotation 448) — a user
    // override on `Promise.resolve` detours to the per-element
    // Call(promiseResolve, C, «v») lane; see `combinator_patched`.
    if unsafe { crate::combinator_patched::consult_active() } {
        return unsafe { crate::combinator_patched::run_all(promises_arr) };
    }
    // An `Array<Any>` input carries NaN-box slots (mixed promise /
    // plain-value elements) — the raw-pointer walk below would
    // dereference immediates; route to the any-lane sibling.
    if unsafe { crate::combinator_any::arr_is_any(promises_arr) } {
        // §27.2.4.1.2 step 5.f and siblings — every element goes
        // through `promiseResolve` BEFORE anything attaches to it, so
        // the walk below sees only promises. Without it a thenable
        // element was taken for an already-fulfilled value and handed
        // on as itself. The patched lane above does the same thing
        // with the user's override; this is its builtin twin.
        return unsafe { crate::combinator_patched::run_all_builtin(promises_arr) };
    }
    // A genuinely pending element is the one thing the walk below
    // cannot answer: it used to reject the result with a placeholder,
    // which is how the commonest shape in the family
    // (`Promise.all([asyncCall(), asyncCall()])`) became an uncaught
    // rejection. The fan-in waits instead. The all-settled input keeps
    // this walk — its microtask position is what the existing fixtures
    // encode, and routing it through jobs would move ticks for nothing.
    if unsafe { crate::combinator_all_fanin::has_pending(promises_arr) } {
        return unsafe { crate::combinator_all_fanin::all_fan_in(promises_arr, target_repr) };
    }
    unsafe { absorb_inputs(promises_arr) };
    let len = unsafe { arr_len(promises_arr) };
    // Pre-scan: first rejected → reject outer with that reason. The
    // element form is read here as well, so the build loop below
    // already knows whether the result array will co-own its slots
    // before it starts filling them. The typed tier guarantees a
    // homogeneous `Array<Promise<T>>` input, so with no form named by
    // the site the elements' forms join (`joined_form`).
    let mut elem_repr = REPR_UNSTAMPED;
    for i in 0..len {
        let pp = unsafe { arr_slot_ptr(promises_arr, i) };
        if pp.is_null() {
            continue;
        }
        let state = unsafe { (*pp).state };
        if state == STATE_REJECTED {
            return unsafe { defer_settle(STATE_REJECTED, (*pp).value, 0, (*pp).value_repr) };
        }
        if elem_repr == REPR_UNSTAMPED {
            elem_repr = crate::settled_form::read_form(unsafe { (*pp).value_repr }, target_repr);
        }
    }
    if target_repr == REPR_UNSTAMPED {
        elem_repr = unsafe { crate::settled_form::joined_form(promises_arr, len, elem_repr) };
    }
    // All fulfilled. The call site typed the result element `any`, so
    // the elements share no raw form and the result has to carry
    // NaN-box slots — the shape an any-shape INPUT already produces,
    // reached here for the other reason (`AllBlock::result_any` names
    // both). Without this the loop below picked ONE element's form and
    // read every slot through it, which is silent for a heterogeneous
    // input: a Str pointer read as a number, `true` read as 1.
    if target_repr == REPR_ANY {
        return unsafe { crate::combinator_any::all_sync_boxed_from_typed(promises_arr, len) };
    }
    let chain = repr_arr_kind_chain(elem_repr);
    // A heap-chained result array DROPS every non-null slot once its
    // last owner dies (`__torajs_arr_drop_heap` walks them all), while
    // each source promise keeps its own stake — so every element the
    // result co-owns has to be paid for here. Without this the sources
    // died first and freed the very cells the result still pointed at,
    // and the reads came back as whatever later reused those blocks:
    // SILENTLY wrong values rather than a crash. `allSettled`'s
    // inner-value inc and `race`/`any`'s forwarded-value inc are the
    // same payment; only this loop was missing it.
    let co_owns = chain == Some(4);
    let mut result_arr = unsafe { __torajs_arr_alloc(len) };
    for i in 0..len {
        let pp = unsafe { arr_slot_ptr(promises_arr, i) };
        let v = if pp.is_null() {
            0
        } else {
            unsafe { crate::settled_form::into_form((*pp).value_repr, elem_repr, (*pp).value) }
        };
        if co_owns && v != 0 {
            unsafe { __torajs_rc_inc(v as *mut c_void) };
        }
        result_arr = unsafe { __torajs_arr_push(result_arr, v) };
    }
    let out_repr = match chain {
        Some(c) => {
            unsafe { __torajs_arr_mark_kind(result_arr, c) };
            REPR_HEAP
        }
        // Unmarkable slots — keep the settled cell loud rather than
        // hand the any lane a misdecoding array.
        None => REPR_UNSTAMPED,
    };
    unsafe { settle_result(len, STATE_FULFILLED, result_arr as i64, 1, out_repr) }
}
