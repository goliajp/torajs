//! `Promise.all<T>(Promise<T>[]) → Promise<T[]>`, the sync kernel over
//! a promises array. Sibling of [`crate::combinator`], which keeps
//! `race` / `any` and the slot accessors every kernel shares.
//!
//! A typed input is read in one walk, which answers every question
//! the lanes are chosen by (an own `then`, a pending element, the first
//! rejected one, the first element's form); the result array is then
//! filled in a second walk that also marks each input handled.

use core::ffi::c_void;

use crate::combinator::{
    absorb_inputs, arr_len, defer_settle, settle_result, sparse_input_rejects,
};
use crate::combinator_all_fanin::MODE_ALL;
use crate::combinator_dyn::all_sync_untargeted;
use crate::layout::{
    ARR_DATA_PTR_OFF, ARR_HEAD_OFF, ARR_LEN_OFF, Promise, REPR_ANY, REPR_BOOL, REPR_F64, REPR_HEAP,
    REPR_I64, REPR_STR, REPR_UNSTAMPED, REPR_VOID, STATE_FULFILLED, STATE_REJECTED,
};

unsafe extern "C" {
    fn __torajs_rc_inc(p: *mut c_void);
    /// torajs-arr — a fresh Array<T> of capacity `initial_cap`, length 0.
    fn __torajs_arr_alloc(initial_cap: u64) -> *mut c_void;
    /// torajs-arr — elem-kind self-description (RFC 20260704 S1);
    /// the result array is any-consumable via the settled cell's
    /// REPR_HEAP stamp, so it needs the mark too.
    fn __torajs_arr_mark_kind(arr: *mut c_void, chain: u64);
}

/// `flags` bit: no object of the program can carry an own `then` (the
/// compiler's `then` closure proof). Mirrored in torajs-core
/// `ssa_lower_call_promise_static.rs`.
pub const ALL_NO_OWN_THEN: i64 = 1;
/// `flags` bit: `Promise.resolve` stays %Promise.resolve% for the whole
/// run (the compiler's `promise_statics_pinned`). Mirrored likewise.
pub const ALL_STATICS_PINNED: i64 = 2;
/// Both proofs at once: the call site names
/// [`__torajs_promise_all_sync_proved`] instead of passing these flags.
pub const ALL_PROVED: i64 = ALL_NO_OWN_THEN | ALL_STATICS_PINNED;

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

/// The first logical slot of a typed array (8B stride, `head` applied).
#[inline(always)]
unsafe fn slots_of(arr: *mut c_void) -> *mut i64 {
    unsafe {
        let bytes = arr as *mut u8;
        let head = *(bytes.add(ARR_HEAD_OFF) as *const u32) as usize;
        let data = *(bytes.add(ARR_DATA_PTR_OFF) as *const *mut i64);
        data.add(head)
    }
}

/// What one walk over a typed promises array found, in index order.
struct Scan {
    /// Some element's expando bag holds a `then` (the walk stops there:
    /// nothing else is asked once this is known).
    overridden: bool,
    pending: bool,
    first_rejected: *mut Promise,
    /// The first element's form other than UNSTAMPED, as read by the
    /// call site's target.
    form: u8,
}

#[inline(always)]
unsafe fn scan(arr: *mut c_void, len: u64, target_repr: u8, probe_then: bool) -> Scan {
    let mut s = Scan {
        overridden: false,
        pending: false,
        first_rejected: core::ptr::null_mut(),
        form: REPR_UNSTAMPED,
    };
    unsafe {
        let slots = slots_of(arr) as *const *mut Promise;
        for i in 0..len as usize {
            let pp = *slots.add(i);
            if pp.is_null() {
                continue;
            }
            if probe_then && crate::combinator_then_override::cell_overrides_then(pp.cast()) {
                s.overridden = true;
                return s;
            }
            let state = (*pp).state;
            if state == STATE_REJECTED {
                if s.first_rejected.is_null() {
                    s.first_rejected = pp;
                }
            } else if state != STATE_FULFILLED {
                s.pending = true;
            }
            if s.form == REPR_UNSTAMPED {
                s.form = crate::settled_form::read_form((*pp).value_repr, target_repr);
            }
        }
    }
    s
}

/// # Safety
/// `promises_arr` is NULL or a live array whose elements are live Promise cells or values of the declared element lane.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_all_sync(
    promises_arr: *mut c_void,
    target_repr: i64,
    flags: i64,
) -> *mut c_void {
    unsafe { all_sync::<false>(promises_arr, target_repr as u8, flags) }
}

/// [`__torajs_promise_all_sync`] under both whole-program proofs
/// ([`ALL_PROVED`]), which the call site alone can vouch for. Nothing
/// here can reach the own-`then` lane or the patched-`resolve` lane, so
/// an artifact whose every `Promise.all` comes through this entry links
/// neither, nor the generic member read and call they stand on.
///
/// # Safety
/// As [`__torajs_promise_all_sync`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_all_sync_proved(
    promises_arr: *mut c_void,
    target_repr: i64,
) -> *mut c_void {
    unsafe { all_sync::<true>(promises_arr, target_repr as u8, ALL_PROVED) }
}

/// The kernel of both entries; `PROVED` drops the two lanes the
/// proofs rule out from the code, not only from the run.
#[inline(always)]
unsafe fn all_sync<const PROVED: bool>(
    promises_arr: *mut c_void,
    target_repr: u8,
    flags: i64,
) -> *mut c_void {
    if promises_arr.is_null() {
        return unsafe { defer_settle(STATE_REJECTED, 0, 0, REPR_VOID) };
    }
    if unsafe { sparse_input_rejects(promises_arr) } {
        return unsafe { defer_settle(STATE_REJECTED, 0, 0, REPR_VOID) };
    }
    // With the `then` closure proof no element can wear an own `then`,
    // so §27.2.4.1.3 step 6.r's Get(nextPromise, "then") is
    // %Promise.prototype.then% on every element — the attach this
    // kernel performs on the cell's state directly.
    let probe_then = !PROVED && flags & ALL_NO_OWN_THEN == 0;
    // With the statics pinned, §27.2.4.1 step 1's GetPromiseResolve reads
    // %Promise.resolve%, and step 6.i's Call of it answers each element
    // itself (§27.2.4.7.1 step 1: its constructor is %Promise%, which the
    // pinned proof leaves unwritten).
    let consult = !PROVED && flags & ALL_STATICS_PINNED == 0;
    // An `Array<Any>` input carries NaN-box slots (mixed promise /
    // plain-value elements) — the raw-pointer walk below would
    // dereference immediates.
    if unsafe { crate::combinator_any::arr_is_any(promises_arr) } {
        return unsafe { all_sync_any::<PROVED>(promises_arr, probe_then, consult) };
    }
    let len = unsafe { arr_len(promises_arr) };
    let s = unsafe { scan(promises_arr, len, target_repr, probe_then) };
    // The lanes in the order they win. An element wearing its own `then`
    // sends the input down the interleaved lane (§27.2.4.1.3 step
    // 6.q-s); see `combinator_then_override`.
    if !PROVED && s.overridden {
        return unsafe {
            crate::combinator_then_override::defer_overridden(
                promises_arr,
                MODE_ALL,
                all_sync_untargeted,
            )
        };
    }
    // §27.2.4 static-slot patch consult (rotation 448) — a user
    // override on `Promise.resolve` detours to the per-element
    // Call(promiseResolve, C, «v») lane; see `combinator_patched`.
    if !PROVED && consult && unsafe { crate::combinator_patched::consult_active() } {
        return unsafe { crate::combinator_patched::run_all(promises_arr) };
    }
    // A genuinely pending element is the one thing this kernel cannot
    // answer: it used to reject the result with a placeholder, which is
    // how the commonest shape in the family
    // (`Promise.all([asyncCall(), asyncCall()])`) became an uncaught
    // rejection. The fan-in waits instead (and marks every input
    // handled). The all-settled input stays here — its microtask
    // position is what the existing fixtures encode, and routing it
    // through jobs would move ticks for nothing.
    if s.pending {
        return unsafe { crate::combinator_all_fanin::all_fan_in(promises_arr, target_repr) };
    }
    // Every input is settled from here on, and each is marked handled on
    // every road out (the attach every element gets, §27.2.4.1.3 step
    // 6.r): the first rejected one rejects the result with its reason.
    if !s.first_rejected.is_null() {
        unsafe {
            absorb_inputs(promises_arr);
            let pp = s.first_rejected;
            return defer_settle(STATE_REJECTED, (*pp).value, 0, (*pp).value_repr);
        }
    }
    // All fulfilled. The typed tier guarantees a homogeneous
    // `Array<Promise<T>>` input, so with no form named by the site the
    // elements' forms join (`joined_form`).
    let mut elem_repr = s.form;
    if target_repr == REPR_UNSTAMPED {
        elem_repr = unsafe { crate::settled_form::joined_form(promises_arr, len, elem_repr) };
    } else if elem_repr == REPR_UNSTAMPED {
        // No element named a form (an empty input): the slots are the
        // site's, so the result carries the site's form. Left unstamped,
        // an `any`-parameter handler refused the empty array.
        elem_repr = target_repr;
    }
    // The call site typed the result element `any`, so the elements
    // share no raw form and the result has to carry NaN-box slots — the
    // shape an any-shape INPUT already produces, reached here for the
    // other reason (`AllBlock::result_any` names both). Without this the
    // build below picked ONE element's form and read every slot through
    // it, which is silent for a heterogeneous input: a Str pointer read
    // as a number, `true` read as 1.
    if target_repr == REPR_ANY {
        unsafe {
            absorb_inputs(promises_arr);
            return crate::combinator_any::all_sync_boxed_from_typed(promises_arr, len);
        }
    }
    unsafe { build_fulfilled(promises_arr, len, elem_repr) }
}

/// The any-shape input: the lanes in the same order, each asked of
/// the NaN-box slots by its own walk.
unsafe fn all_sync_any<const PROVED: bool>(
    arr: *mut c_void,
    probe_then: bool,
    consult: bool,
) -> *mut c_void {
    unsafe {
        if !PROVED
            && probe_then
            && let Some(out) = crate::combinator_then_override::defer_on_then_override(
                arr,
                MODE_ALL,
                all_sync_untargeted,
            )
        {
            return out;
        }
        if !PROVED && consult && crate::combinator_patched::consult_active() {
            return crate::combinator_patched::run_all(arr);
        }
        // §27.2.4.1.2 step 5.f and siblings — every element goes
        // through `promiseResolve` BEFORE anything attaches to it, so
        // the walk sees only promises. Without it a thenable element
        // was taken for an already-fulfilled value and handed on as
        // itself. The patched lane above does the same thing with the
        // user's override; this is its builtin twin.
        if PROVED {
            crate::combinator_patched::run_all_proved(arr)
        } else {
            crate::combinator_patched::run_all_builtin(arr)
        }
    }
}

/// The result of an all-fulfilled typed input: each value in form
/// `elem_repr`, written straight into a fresh array of exactly `len`
/// slots, with each input marked handled on the way.
unsafe fn build_fulfilled(arr: *mut c_void, len: u64, elem_repr: u8) -> *mut c_void {
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
    unsafe {
        let result = __torajs_arr_alloc(len);
        // a fresh array: head 0, capacity `len`, nothing for a guard to refuse
        let out = slots_of(result);
        let src = slots_of(arr) as *const *mut Promise;
        for i in 0..len as usize {
            let pp = *src.add(i);
            let v = if pp.is_null() {
                0
            } else {
                (*pp).has_handler = 1;
                crate::settled_form::into_form((*pp).value_repr, elem_repr, (*pp).value)
            };
            if co_owns && v != 0 {
                __torajs_rc_inc(v as *mut c_void);
            }
            *out.add(i) = v;
        }
        *((result as *mut u8).add(ARR_LEN_OFF) as *mut u64) = len;
        let out_repr = match chain {
            Some(c) => {
                __torajs_arr_mark_kind(result, c);
                REPR_HEAP
            }
            // Unmarkable slots — keep the settled cell loud rather than
            // hand the any lane a misdecoding array.
            None => REPR_UNSTAMPED,
        };
        settle_result(len, STATE_FULFILLED, result as i64, 1, out_repr)
    }
}
