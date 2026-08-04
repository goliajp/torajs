//! `Promise.all` over an `Array<Any>` input (mixed promise /
//! plain-value elements) — the any-lane sibling of
//! [`crate::combinator`]'s typed fast path.
//!
//! §27.2.4.1 PerformPromiseAll treats a non-thenable element as an
//! already-fulfilled value (`Promise.resolve(x)` wrap); the typed
//! tier never sees that shape (checker admits homogeneous
//! `Array<Promise<T>>` only), but `Promise.all([Promise.resolve(1),
//! 2])` infers `Array<Any>` whose slots are NaN-box bits — the
//! typed kernel's raw-pointer slot walk would dereference an int32
//! immediate. The entry kernel gates on `FLAG_ARR_ANY` and routes
//! here.
//!
//! Element classification per slot:
//! - NaN-box heap cell with `TAG_PROMISE` → a promise element:
//!   settle state / repr / value read off the cell (mint sites
//!   stamped the repr when the promise crossed into `any`).
//! - everything else (immediates, non-promise cells) → an
//!   already-fulfilled value, stored verbatim.
//!
//! Result: an `Array<Any>` (NaN-box slots, self-describing) inside
//! a `REPR_HEAP` result promise. Same MVP posture as the typed
//! path: a PENDING element (or an UNSTAMPED promise element — a
//! mint family without repr wiring) rejects the outer with the
//! placeholder reason instead of mis-boxing.

use core::ffi::c_void;

use crate::layout::{
    ARR_DATA_PTR_OFF, ARR_HEAD_OFF, ARR_LEN_OFF, Promise, REPR_ANY, REPR_BOOL, REPR_F64, REPR_HEAP,
    REPR_I64, REPR_NULL, REPR_STR, REPR_UNSTAMPED, REPR_VOID, STATE_FULFILLED, STATE_PENDING,
    STATE_REJECTED, TAG_PROMISE,
};

/// `FLAG_ARR_ANY` mirror (torajs-rc `flags.rs`, lockstep).
pub(crate) const FLAG_ARR_ANY: u16 = 1 << 3;

unsafe extern "C" {
    /// torajs-arr — `new Array(n)` any-shape alloc (len=cap=n,
    /// undefined-filled); the build loop overwrites every slot.
    fn __torajs_arr_alloc_any_filled(n: u64) -> *mut u8;
    /// torajs-anyvalue — NaN-box pack / Str-slot boxer / NaN-box-
    /// aware share (immediates no-op).
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
    fn __torajs_anyv_box_str_slot(s: *mut c_void) -> u64;
    fn __torajs_anyv_rc_inc(v: u64);
    /// torajs-rc — raw cell share (the race fan-in pays for the stake
    /// each adopt job consumes).
    fn __torajs_rc_inc(p: *mut c_void);
    /// torajs-arr raw-slot alloc + push + elem-kind mark — the
    /// allSettled result is a typed heap-cell array of `{status,
    /// value}` structs (the typed kernel's result shape), not an
    /// any-shape array.
    fn __torajs_arr_alloc(initial_cap: u64) -> *mut c_void;
    fn __torajs_arr_push(arr: *mut c_void, val: i64) -> *mut c_void;
    fn __torajs_arr_mark_kind(arr: *mut c_void, chain: u64);
}

/// True when the input array carries NaN-box slots.
pub(crate) unsafe fn arr_is_any(arr: *mut c_void) -> bool {
    unsafe { (*((arr as *mut u8).add(6) as *const u16)) & FLAG_ARR_ANY != 0 }
}

/// Raw NaN-box bits of logical slot `i` (8B stride behind the data
/// pointer — the combinator `arr_slot_ptr` walk, bits not pointer).
pub(crate) unsafe fn any_slot(arr: *mut c_void, i: u64) -> u64 {
    unsafe {
        let bytes = arr as *mut u8;
        let head = *(bytes.add(ARR_HEAD_OFF) as *const u32) as u64;
        let data = *(bytes.add(ARR_DATA_PTR_OFF) as *const *mut u8);
        *(data.add(((head + i) * 8) as usize) as *const u64)
    }
}

/// NaN-box cell gate (mirror of `torajs-anyvalue::nanbox::is_cell`)
/// + promise tag probe. `None` = not a promise element.
pub(crate) unsafe fn slot_promise(bits: u64) -> Option<*mut Promise> {
    const TOP_16_MASK: u64 = 0xFFFF_0000_0000_0000;
    const TAG_BIT_TYPE_OTHER: u64 = 0x02;
    if bits == 0 || bits & TOP_16_MASK != 0 || bits & TAG_BIT_TYPE_OTHER != 0 {
        return None;
    }
    let p = bits as *mut u8;
    if unsafe { *(p.add(4) as *const u16) } != TAG_PROMISE {
        return None;
    }
    Some(p as *mut Promise)
}

/// The undefined box — the total-function filler for a slot with no
/// form to describe it (the sync siblings gate those out up front).
pub(crate) unsafe fn box_undefined() -> u64 {
    unsafe { __torajs_anyv_box_from_pair(5, 0) }
}

/// An any-shape result array, `len` slots prefilled with undefined —
/// the fan-in overwrites each as its element reports in.
pub(crate) unsafe fn alloc_any_result(len: u64) -> *mut c_void {
    unsafe { __torajs_arr_alloc_any_filled(len) as *mut c_void }
}

/// Share one more owner of a NaN-box slot (immediates no-op).
pub(crate) unsafe fn box_share(bits: u64) {
    unsafe { __torajs_anyv_rc_inc(bits) };
}

/// Box a settled promise slot per its repr stamp, OWNED protocol —
/// the result array keeps its own stake (`anyv_rc_inc` is NaN-box
/// aware, immediates pass through). `None` = UNSTAMPED (caller
/// rejects with the MVP placeholder instead of mis-boxing).
pub(crate) unsafe fn box_settled_owned(repr: u8, value: i64) -> Option<u64> {
    let boxed = unsafe {
        match repr {
            REPR_I64 => __torajs_anyv_box_from_pair(2, value),
            REPR_F64 => __torajs_anyv_box_from_pair(3, value),
            REPR_BOOL => __torajs_anyv_box_from_pair(1, value),
            REPR_STR => __torajs_anyv_box_str_slot(value as *mut c_void),
            REPR_HEAP => __torajs_anyv_box_from_pair(4, value),
            REPR_ANY => value as u64,
            REPR_VOID => __torajs_anyv_box_from_pair(5, 0),
            REPR_NULL => __torajs_anyv_box_from_pair(0, 0),
            REPR_UNSTAMPED => return None,
            _ => return None,
        }
    };
    unsafe { __torajs_anyv_rc_inc(boxed) };
    Some(boxed)
}

/// The `Array<Any>` input walk — see module doc. Pre-scan settles
/// the rejection/pending verdict first (spec order: first rejection
/// wins), then the build loop boxes every fulfilled value into the
/// result `Array<Any>`.
pub(crate) unsafe fn all_sync_any(promises_arr: *mut c_void) -> *mut c_void {
    unsafe {
        // A pending element gets waited on, exactly as on the typed
        // lane. Leaving this one behind would make "does a pending
        // element get waited on" depend on whether the array literal
        // happened to infer `Array<Any>` — the same lane asymmetry the
        // reaction handlers had.
        if crate::combinator_all_fanin::has_pending_any(promises_arr) {
            return crate::combinator_all_fanin::all_fan_in_any(promises_arr);
        }
        let len = *((promises_arr as *mut u8).add(ARR_LEN_OFF) as *const u64);
        // Absorb + pre-scan in one walk (the typed path's
        // `absorb_inputs` shape — every promise element counts as
        // handled per the per-element handler attach the spec does).
        for i in 0..len {
            let Some(pp) = slot_promise(any_slot(promises_arr, i)) else {
                continue;
            };
            (*pp).has_handler = 1;
            match (*pp).state {
                STATE_REJECTED => {
                    let Some(reason) = box_settled_owned((*pp).value_repr, (*pp).value) else {
                        return crate::combinator::defer_settle(STATE_REJECTED, 0, 0, REPR_VOID);
                    };
                    return crate::combinator::defer_settle(
                        STATE_REJECTED,
                        reason as i64,
                        1,
                        REPR_ANY,
                    );
                }
                STATE_PENDING => {
                    // MVP sync fast path — no fan-in yet (the typed
                    // path's placeholder posture).
                    return crate::combinator::defer_settle(STATE_REJECTED, 0, 0, REPR_VOID);
                }
                // Fulfilled — gate UNSTAMPED here so the build loop
                // below never aborts mid-array (which would strand
                // the half-boxed result).
                _ if (*pp).value_repr == REPR_UNSTAMPED => {
                    return crate::combinator::defer_settle(STATE_REJECTED, 0, 0, REPR_VOID);
                }
                _ => {}
            }
        }
        // All fulfilled — build the NaN-box result array.
        let out = __torajs_arr_alloc_any_filled(len);
        let head = *(out.add(ARR_HEAD_OFF) as *const u32) as u64;
        let data = *(out.add(ARR_DATA_PTR_OFF) as *const *mut u8);
        for i in 0..len {
            let bits = any_slot(promises_arr, i);
            let v = match slot_promise(bits) {
                // The pre-scan gated UNSTAMPED — the None arm is
                // unreachable; undefined keeps it total without a
                // runtime panic path.
                Some(pp) => box_settled_owned((*pp).value_repr, (*pp).value)
                    .unwrap_or_else(|| __torajs_anyv_box_from_pair(5, 0)),
                None => {
                    // Plain value — stored verbatim, one more owner.
                    __torajs_anyv_rc_inc(bits);
                    bits
                }
            };
            *(data.add(((head + i) * 8) as usize) as *mut u64) = v;
        }
        crate::combinator::settle_result(len, STATE_FULFILLED, out as i64, 1, REPR_HEAP)
    }
}

/// `Promise.allSettled` over an `Array<Any>` (§27.2.4.3). Every
/// element lands in the result as a settled record — `{status, value}`
/// when fulfilled, `{status, reason}` when rejected, which is why
/// `record_tags` carries a stamp for each shape — and a rejected
/// element does NOT short-circuit. The struct's second slot
/// holds boxed AnyValue bits; the checker types the result element
/// `{status: string, value: any}` so field reads decode the NaN-box.
/// The result is a typed heap-cell array (chain 4) of struct
/// pointers — exactly the typed kernel's result shape.
pub(crate) unsafe fn allsettled_sync_any(
    promises_arr: *mut c_void,
    record_tags: u64,
) -> *mut c_void {
    unsafe {
        if crate::combinator_all_fanin::has_pending_any(promises_arr) {
            return crate::combinator_all_fanin::allsettled_fan_in_any(promises_arr, record_tags);
        }
        let len = *((promises_arr as *mut u8).add(ARR_LEN_OFF) as *const u64);
        // Absorb + verdict in one walk: a PENDING element (MVP — no
        // fan-in yet) or an UNSTAMPED settled promise element (mint
        // family without repr wiring) rejects up front so the build
        // loop below never aborts mid-array (all_sync_any's gate
        // posture; allSettled has no first-rejection exit).
        for i in 0..len {
            let Some(pp) = slot_promise(any_slot(promises_arr, i)) else {
                continue;
            };
            (*pp).has_handler = 1;
            if (*pp).state == STATE_PENDING || (*pp).value_repr == REPR_UNSTAMPED {
                return crate::combinator::defer_settle(STATE_REJECTED, 0, 0, REPR_VOID);
            }
        }
        // Build — every element becomes a settled struct; a boxed
        // settle value transfers its fresh stake into the struct's
        // value slot (box_settled_owned inc'd it), a plain value
        // takes one more owner (typed kernel's value_is_heap inc).
        let mut result_arr = __torajs_arr_alloc(len);
        for i in 0..len {
            let bits = any_slot(promises_arr, i);
            let s = match slot_promise(bits) {
                // The pre-scan gated UNSTAMPED — the None arm is
                // unreachable; undefined keeps it total without a
                // runtime panic path.
                Some(pp) => {
                    let v = box_settled_owned((*pp).value_repr, (*pp).value)
                        .unwrap_or_else(|| __torajs_anyv_box_from_pair(5, 0));
                    crate::combinator_allsettled::alloc_settled_struct(
                        (*pp).state,
                        v as i64,
                        record_tags,
                    )
                }
                None => {
                    // Plain value — already fulfilled (§27.2.4.3
                    // resolve-wrap), stored boxed verbatim.
                    __torajs_anyv_rc_inc(bits);
                    crate::combinator_allsettled::alloc_settled_struct(
                        STATE_FULFILLED,
                        bits as i64,
                        record_tags,
                    )
                }
            };
            result_arr = __torajs_arr_push(result_arr, s as i64);
        }
        // {status, value} obj cells — heap-ptr slots (chain 4).
        __torajs_arr_mark_kind(result_arr, 4);
        crate::combinator::settle_result(len, STATE_FULFILLED, result_arr as i64, 1, REPR_HEAP)
    }
}

/// Mark every promise element handled (the typed path's
/// `absorb_inputs` shape) — race / any settle on the first winner and
/// return early, so without an up-front pass a later rejected element
/// would reach the HPRT-check microtask with `has_handler = 0` and
/// spuriously report an unhandled rejection (§27.2.4.{2,5} attach a
/// reject-element handler to every input).
pub(crate) unsafe fn absorb_any_inputs(promises_arr: *mut c_void) {
    unsafe {
        let len = *((promises_arr as *mut u8).add(ARR_LEN_OFF) as *const u64);
        for i in 0..len {
            if let Some(pp) = slot_promise(any_slot(promises_arr, i)) {
                (*pp).has_handler = 1;
            }
        }
    }
}

/// `Promise.race` over an `Array<Any>` (§27.2.4.5). First settled
/// element in array order wins (the typed `race_sync` MVP posture —
/// no real-time fan-in); a non-promise element is an already-fulfilled
/// value and wins on sight. The winning value rides a `REPR_ANY`
/// result cell (single boxed AnyValue, is_heap=1 so `value_drop_heap`'s
/// NaN-box gate drops it correctly whether immediate or heap).
pub(crate) unsafe fn race_sync_any(promises_arr: *mut c_void) -> *mut c_void {
    unsafe {
        let len = *((promises_arr as *mut u8).add(ARR_LEN_OFF) as *const u64);
        absorb_any_inputs(promises_arr);
        for i in 0..len {
            let bits = any_slot(promises_arr, i);
            let Some(pp) = slot_promise(bits) else {
                // Plain value — already fulfilled, first one wins.
                __torajs_anyv_rc_inc(bits);
                return crate::combinator::defer_settle(STATE_FULFILLED, bits as i64, 1, REPR_ANY);
            };
            (*pp).has_handler = 1;
            let state = (*pp).state;
            if state == STATE_FULFILLED || state == STATE_REJECTED {
                let Some(v) = box_settled_owned((*pp).value_repr, (*pp).value) else {
                    return crate::combinator::defer_settle(STATE_REJECTED, 0, 0, REPR_VOID);
                };
                return crate::combinator::defer_settle(state, v as i64, 1, REPR_ANY);
            }
            // PENDING — skip (array-order first-settled).
        }
        // Nothing settled yet — fan in, exactly as the typed sibling
        // does (see `combinator::race_fan_in`): one adopt job per
        // element, first settlement wins, an empty iterable answers a
        // forever-pending promise per §27.2.4.5 step 3.
        let result = crate::pool::__torajs_promise_alloc_pending();
        let rp = crate::layout::as_promise(result);
        // Empty iterable — never settles, so nothing to mis-box; see
        // the typed sibling's note on why VOID and not UNSTAMPED.
        (*rp).value_repr = REPR_VOID;
        let mut stamped = false;
        for i in 0..len {
            let Some(pp) = slot_promise(any_slot(promises_arr, i)) else {
                continue;
            };
            if !stamped {
                (*rp).value_repr = (*pp).value_repr;
                (*rp).value_is_heap = (*pp).value_is_heap;
                stamped = true;
            }
            __torajs_rc_inc(pp as *mut c_void);
            crate::then_adopt::adopt_into(result, pp as *mut c_void);
        }
        result
    }
}

/// `Promise.any` over an `Array<Any>` (§27.2.4.2). First FULFILLED
/// element wins; all-rejected answers an AggregateError over every
/// reason. A rejected element's `(repr, value)` is borrowed off the
/// still-live input array and boxed only where it is kept, so no owned
/// rejection is stranded when a later element fulfills.
pub(crate) unsafe fn any_sync_any(promises_arr: *mut c_void) -> *mut c_void {
    unsafe {
        // A pending element gets waited on, exactly as on the typed
        // lane — leaving this behind would make "does a pending
        // element get waited on" depend on whether the array literal
        // happened to infer `Array<Any>`.
        if crate::combinator_all_fanin::has_pending_any(promises_arr) {
            return crate::combinator_all_fanin::any_fan_in_any(promises_arr);
        }
        let len = *((promises_arr as *mut u8).add(ARR_LEN_OFF) as *const u64);
        absorb_any_inputs(promises_arr);
        let mut have_rej = false;
        let mut last_rej_repr: u8 = REPR_VOID;
        let mut last_rej_value: i64 = 0;
        for i in 0..len {
            let bits = any_slot(promises_arr, i);
            let Some(pp) = slot_promise(bits) else {
                // Plain value — already fulfilled, first fulfilled wins.
                __torajs_anyv_rc_inc(bits);
                return crate::combinator::defer_settle(STATE_FULFILLED, bits as i64, 1, REPR_ANY);
            };
            (*pp).has_handler = 1;
            match (*pp).state {
                STATE_FULFILLED => {
                    let Some(v) = box_settled_owned((*pp).value_repr, (*pp).value) else {
                        return crate::combinator::defer_settle(STATE_REJECTED, 0, 0, REPR_VOID);
                    };
                    return crate::combinator::defer_settle(STATE_FULFILLED, v as i64, 1, REPR_ANY);
                }
                STATE_REJECTED => {
                    have_rej = true;
                    last_rej_repr = (*pp).value_repr;
                    last_rej_value = (*pp).value;
                }
                _ => {} // PENDING — the gate above ruled these out.
            }
        }
        // Every element rejected — §27.2.4.2's AggregateError, the
        // typed sibling's answer verbatim.
        if let Some(err) = any_aggregate_error(promises_arr, len) {
            return crate::combinator::settle_result(len, STATE_REJECTED, err as i64, 1, REPR_HEAP);
        }
        if have_rej {
            let Some(v) = box_settled_owned(last_rej_repr, last_rej_value) else {
                return crate::combinator::defer_settle(STATE_REJECTED, 0, 0, REPR_VOID);
            };
            return crate::combinator::defer_settle(STATE_REJECTED, v as i64, 1, REPR_ANY);
        }
        // Empty or all-pending — placeholder reject.
        crate::combinator::defer_settle(STATE_REJECTED, 0, 0, REPR_VOID)
    }
}

/// The any-lane form of `combinator::any_aggregate_error`: the reasons
/// are read off boxed slots, and a non-promise element is an
/// already-fulfilled value that never reaches here.
unsafe fn any_aggregate_error(promises_arr: *mut c_void, len: u64) -> Option<*mut c_void> {
    unsafe {
        let errors = crate::combinator_aggregate::alloc_errors(len);
        for i in 0..len {
            let Some(pp) = slot_promise(any_slot(promises_arr, i)) else {
                continue;
            };
            let boxed =
                box_settled_owned((*pp).value_repr, (*pp).value).unwrap_or_else(|| box_undefined());
            crate::combinator_aggregate::store_error(errors, i, boxed);
        }
        crate::combinator_aggregate::make(errors)
    }
}
