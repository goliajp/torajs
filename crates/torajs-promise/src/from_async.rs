//! `Array.fromAsync(items)` — proposal-array-from-async §2.1.1,
//! sync-source MVP.
//!
//! The dynamic entry drives `__torajs_any_iter_next_array_like`
//! (the `Array.from` step kernel: a spec-iterable walks its
//! GetIterator protocol, anything else takes the §23.1.2.1 step-3
//! array-like branch — `length` + index keys, so a plain number
//! answers `[]` and `null` throws the length-read TypeError), then
//! settles each collected element: spec step 5.e awaits every
//! element, so a settled promise element unwraps to its value per
//! its repr stamp and a rejected one rejects the whole result. The
//! result rides a `REPR_HEAP` promise holding an `Array<Any>`.
//!
//! The mapped form (`__torajs_array_from_async_map_dyn`) interleaves
//! per element exactly as the spec does: await the element, call
//! `mapfn(value, k)` through the uniform boxed-adapter ABI, await
//! the mapped result; step 2's IsCallable check precedes iteration.
//!
//! Recorded MVP boundaries (registered, spec-strict forms follow):
//! a real async iterator (`Symbol.asyncIterator`) is not consulted —
//! the sync step protocol answers for it; a PENDING or UNSTAMPED
//! promise element takes the combinator placeholder-reject posture;
//! non-promise thenables store verbatim (no generic `then` await);
//! `thisArg` typecheck-and-drops (tr closures don't bind `this`).

use core::ffi::c_void;

use crate::combinator_any::{box_settled_owned, slot_promise};
use crate::combinator_dyn::collect_items;
use crate::layout::{
    ARR_DATA_PTR_OFF, ARR_HEAD_OFF, REPR_ANY, REPR_HEAP, REPR_UNSTAMPED, REPR_VOID,
    STATE_FULFILLED, STATE_PENDING, STATE_REJECTED,
};

unsafe extern "C" {
    fn __torajs_any_iter_next_array_like(
        recv: u64,
        idx_slot: *mut i64,
        iter_slot: *mut u64,
        out: *mut u64,
    ) -> i64;
    fn __torajs_arr_alloc_any_filled(n: u64) -> *mut u8;
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
    fn __torajs_anyv_rc_dec(v: u64);
    /// torajs-anyvalue — uniform boxed-adapter call over an
    /// any-held callee (argv slots BORROWED, return caller-owned;
    /// non-callable mints a catchable TypeError).
    fn __torajs_any_call(recv: u64, argv: *const u64, argc: i64) -> u64;
    /// torajs-anyvalue — the explicit-`this` twin: §2.1.1 step
    /// 3.j.ii.6.a is `Call(mapfn, thisArg, «value, k»)`, so the
    /// mapped kernel hands its receiver through (borrowed, like
    /// the argv slots).
    fn __torajs_any_call_with_this(recv: u64, this_arg: u64, argv: *const u64, argc: i64) -> u64;
    /// torajs-anyvalue — §7.2.4 IsConstructor over an any box.
    fn __torajs_is_constructor(v: u64) -> bool;
    fn __torajs_anyv_rc_inc(v: u64);
    /// torajs-anyvalue — the constructor-`this` finish kernel:
    /// Construct + define walk + length over the settled boxes
    /// (OWNED, transferred in). Undefined + pending throw on abrupt.
    fn __torajs_from_async_construct_finish(
        this_c: u64,
        items: u64,
        elems: *const u64,
        n: i64,
    ) -> u64;
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    fn __torajs_throw_check() -> i64;
}

/// Callback-shape probe — NaN-box cell gate + Closure tag + boxed
/// dual entry present. Mirrors torajs-anyvalue
/// `closure_boxed_entry` (offsets lockstep torajs-core
/// `CLOSURE_BOXED_ENTRY_OFF`); needed here because §2.1.1 step 2
/// checks IsCallable BEFORE iteration — a non-callable mapfn
/// rejects even for an empty source, so the per-call TypeError
/// inside `__torajs_any_call` fires too late.
unsafe fn callback_shaped(bits: u64) -> bool {
    const TOP_16_MASK: u64 = 0xFFFF_0000_0000_0000;
    const TAG_BIT_TYPE_OTHER: u64 = 0x02;
    const TAG_CLOSURE: u16 = 3;
    const CLOSURE_BOXED_ENTRY_OFF: usize = 32;
    if bits == 0 || bits & TOP_16_MASK != 0 || bits & TAG_BIT_TYPE_OTHER != 0 {
        return false;
    }
    unsafe {
        let p = bits as *mut u8;
        if *(p.add(4) as *const u16) != TAG_CLOSURE {
            return false;
        }
        *(p.add(CLOSURE_BOXED_ENTRY_OFF) as *const u64) != 0
    }
}

/// # Safety
/// `v` is a live any-boxed value the caller owns for the duration
/// of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_array_from_async_dyn(v: u64) -> *mut c_void {
    unsafe {
        let items = match collect_items(v, __torajs_any_iter_next_array_like) {
            Err(rejected) => return rejected,
            Ok(items) => items,
        };
        // Pre-scan (spec step 5.e award order): the first rejected
        // element's reason rejects the whole result; a PENDING or
        // UNSTAMPED element takes the MVP placeholder reject. Both
        // verdicts release every collected item before answering,
        // so the build loop below never strands a half-owned Vec.
        for &bits in &items {
            let Some(pp) = slot_promise(bits) else {
                continue;
            };
            (*pp).has_handler = 1;
            let state = (*pp).state;
            let verdict = if state == STATE_REJECTED {
                match box_settled_owned((*pp).value_repr, (*pp).value) {
                    Some(reason) => {
                        crate::combinator::defer_settle(STATE_REJECTED, reason as i64, 1, REPR_ANY)
                    }
                    None => crate::combinator::defer_settle(STATE_REJECTED, 0, 0, REPR_VOID),
                }
            } else if state == STATE_PENDING || (*pp).value_repr == REPR_UNSTAMPED {
                crate::combinator::defer_settle(STATE_REJECTED, 0, 0, REPR_VOID)
            } else {
                continue;
            };
            for it in items {
                __torajs_anyv_rc_dec(it);
            }
            return verdict;
        }
        // All plain or fulfilled — build the Array<Any> result. A
        // fulfilled promise element unwraps to a fresh owned box and
        // the collected promise reference releases; a plain element
        // transfers its collected stake verbatim.
        let out = __torajs_arr_alloc_any_filled(items.len() as u64);
        let head = *(out.add(ARR_HEAD_OFF) as *const u32) as u64;
        let data = *(out.add(ARR_DATA_PTR_OFF) as *const *mut u8);
        for (i, bits) in items.into_iter().enumerate() {
            let v = match slot_promise(bits) {
                // The pre-scan gated UNSTAMPED — the fallback keeps
                // this total without a runtime panic path (the
                // combinator_any build-loop posture).
                Some(pp) => {
                    let unwrapped = box_settled_owned((*pp).value_repr, (*pp).value)
                        .unwrap_or_else(|| __torajs_anyv_box_from_pair(5, 0));
                    __torajs_anyv_rc_dec(bits);
                    unwrapped
                }
                None => bits,
            };
            *(data.add((head as usize + i) * 8) as *mut u64) = v;
        }
        crate::combinator::defer_settle(STATE_FULFILLED, out as i64, 1, REPR_HEAP)
    }
}

/// `Array.fromAsync.call(C, items)` — the constructor-`this` face
/// (proposal §2.1.1 steps 3.j / 3.k.iv; RFC 20260808 B6 刀 4). A
/// non-constructor `this` (undefined on a bare call) takes the plain
/// ArrayCreate kernel unchanged; a constructor settles every element
/// exactly like the plain form, then hands the owned boxes to the
/// anyvalue finish kernel (Construct + CreateDataPropertyOrThrow walk
/// + `length`) and fulfills with its product.
///
/// # Safety
/// `this_c` and `v` are live any-boxed values the caller owns for
/// the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_array_from_async_this_dyn(this_c: u64, v: u64) -> *mut c_void {
    unsafe {
        if !__torajs_is_constructor(this_c) {
            return __torajs_array_from_async_dyn(v);
        }
        let items = match collect_items(v, __torajs_any_iter_next_array_like) {
            Err(rejected) => return rejected,
            Ok(items) => items,
        };
        let mut settled: Vec<u64> = Vec::with_capacity(items.len());
        let mut verdict: Option<*mut c_void> = None;
        for &bits in &items {
            match settle_slot(bits) {
                Ok((elem, owned)) => {
                    // The finish kernel consumes OWNED boxes; a plain
                    // element's collected stake transfers below (the
                    // items loop skips its release), an unwrapped one
                    // is already fresh.
                    settled.push(elem);
                    if !owned {
                        // Transferred verbatim — do not release with
                        // the items sweep. Mark by leaking the items
                        // stake: swap in a second stake instead.
                        __torajs_anyv_rc_inc(bits);
                    }
                }
                Err(rejected) => {
                    verdict = Some(rejected);
                    break;
                }
            }
        }
        for it in items {
            __torajs_anyv_rc_dec(it);
        }
        if let Some(rejected) = verdict {
            for s in settled {
                __torajs_anyv_rc_dec(s);
            }
            return rejected;
        }
        let n = settled.len() as i64;
        let product = __torajs_from_async_construct_finish(this_c, v, settled.as_ptr(), n);
        if __torajs_throw_check() != 0 {
            return crate::combinator_dyn::reject_with_pending_throw();
        }
        crate::combinator::defer_settle(STATE_FULFILLED, product as i64, 1, REPR_ANY)
    }
}

/// Settle one collected slot: a fulfilled promise unwraps to a
/// fresh OWNED box, a plain value answers a BORROW of the
/// collected stake (`owned = false`); a rejected / pending /
/// unstamped promise answers the verdict promise instead.
unsafe fn settle_slot(bits: u64) -> Result<(u64, bool), *mut c_void> {
    unsafe {
        let Some(pp) = slot_promise(bits) else {
            return Ok((bits, false));
        };
        (*pp).has_handler = 1;
        let state = (*pp).state;
        if state == STATE_REJECTED {
            return Err(match box_settled_owned((*pp).value_repr, (*pp).value) {
                Some(reason) => {
                    crate::combinator::defer_settle(STATE_REJECTED, reason as i64, 1, REPR_ANY)
                }
                None => crate::combinator::defer_settle(STATE_REJECTED, 0, 0, REPR_VOID),
            });
        }
        if state == STATE_PENDING || (*pp).value_repr == REPR_UNSTAMPED {
            return Err(crate::combinator::defer_settle(
                STATE_REJECTED,
                0,
                0,
                REPR_VOID,
            ));
        }
        let unwrapped = box_settled_owned((*pp).value_repr, (*pp).value)
            .unwrap_or_else(|| __torajs_anyv_box_from_pair(5, 0));
        Ok((unwrapped, true))
    }
}

/// §7.4.9 IteratorClose driven by an abrupt completion that is
/// already a REJECTED verdict promise rather than a pending throw —
/// the iterator's `return()` runs and anything IT throws is
/// swallowed (step 6: the original completion wins).
unsafe fn close_swallow(iter_slot: u64) {
    unsafe extern "C" {
        fn __torajs_iter_close_value(iter: u64);
        fn __torajs_throw_take() -> i64;
        fn __torajs_throw_take_tag() -> i64;
    }
    unsafe {
        __torajs_iter_close_value(iter_slot);
        if __torajs_throw_check() != 0 {
            let tag = __torajs_throw_take_tag();
            let val = __torajs_throw_take();
            __torajs_anyv_rc_dec(__torajs_anyv_box_from_pair(tag, val));
        }
    }
}

/// `Array.fromAsync(items, mapfn)` — the mapped form, iterating the
/// spec's way (proposal §2.1.1 step 3.j): ONE loop in which element
/// k awaits (settled unwrap), `mapfn(value, k)` runs, and its result
/// awaits — so an infinite iterable is aborted by the first abrupt
/// element rather than collected forever. A mapfn throw
/// (3.j.ii.6.b IfAbruptCloseAsyncIterator), a rejected element
/// (3.j.ii.5) or a rejected mapped value CLOSES the iterator —
/// `return()` runs once, its own throw swallowed — and answers the
/// rejection.
///
/// `this_arg` is the §2.1.1 step 3.j.ii.6.a receiver — every
/// `Call(mapfn, thisArg, «value, k»)` hands it through (the
/// undefined box when the call site passed none).
///
/// # Safety
/// `v`, `cb` and `this_arg` are live any-boxed values the caller
/// owns for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_array_from_async_map_dyn(
    v: u64,
    cb: u64,
    this_arg: u64,
) -> *mut c_void {
    unsafe {
        unsafe extern "C" {
            fn __torajs_iter_close_abrupt(iter: u64);
        }
        // §2.1.1 step 2 — IsCallable precedes iteration.
        if !callback_shaped(cb) {
            __torajs_throw_type_error(c"mapfn is not a function".as_ptr());
            return crate::combinator_dyn::reject_with_pending_throw();
        }
        let undef = __torajs_anyv_box_from_pair(5, 0);
        let mut idx: i64 = 0;
        let mut iter_slot: u64 = undef;
        let mut out_v: u64 = undef;
        let mut mapped: Vec<u64> = Vec::new();
        let mut verdict: Option<*mut c_void> = None;
        let mut i: usize = 0;
        loop {
            let has = __torajs_any_iter_next_array_like(v, &mut idx, &mut iter_slot, &mut out_v);
            if __torajs_throw_check() != 0 {
                // The step's own abrupt — [[Done]] is true, no close.
                verdict = Some(crate::combinator_dyn::reject_with_pending_throw());
                break;
            }
            if has == 0 {
                break;
            }
            let bits = out_v;
            let (elem, elem_owned) = match settle_slot(bits) {
                Ok(pair) => pair,
                Err(rejected) => {
                    // 3.j.ii.5 — the element await's abrupt closes.
                    __torajs_anyv_rc_dec(bits);
                    close_swallow(iter_slot);
                    verdict = Some(rejected);
                    break;
                }
            };
            // argv slots are borrows; the index is an immediate.
            let argv = [elem, __torajs_anyv_box_from_pair(2, i as i64)];
            i += 1;
            let ret = __torajs_any_call_with_this(cb, this_arg, argv.as_ptr(), 2);
            if elem_owned {
                __torajs_anyv_rc_dec(elem);
            }
            __torajs_anyv_rc_dec(bits);
            if __torajs_throw_check() != 0 {
                // 3.j.ii.6.b — the mapfn's abrupt closes, original
                // completion first (stash-close-restore).
                __torajs_iter_close_abrupt(iter_slot);
                verdict = Some(crate::combinator_dyn::reject_with_pending_throw());
                break;
            }
            // step 3.j.ii.7 — the mapped value awaits too.
            match settle_slot(ret) {
                Ok((mv, owned)) => {
                    if owned {
                        // Unwrapped from the returned promise — the
                        // promise cell's own stake releases.
                        __torajs_anyv_rc_dec(ret);
                        mapped.push(mv);
                    } else {
                        // Plain return — `ret` is already the
                        // caller-owned box, transfer it.
                        mapped.push(ret);
                    }
                }
                Err(rejected) => {
                    __torajs_anyv_rc_dec(ret);
                    close_swallow(iter_slot);
                    verdict = Some(rejected);
                    break;
                }
            }
        }
        __torajs_anyv_rc_dec(iter_slot);
        if let Some(rejected) = verdict {
            for m in mapped {
                __torajs_anyv_rc_dec(m);
            }
            return rejected;
        }
        let out = __torajs_arr_alloc_any_filled(mapped.len() as u64);
        let head = *(out.add(ARR_HEAD_OFF) as *const u32) as u64;
        let data = *(out.add(ARR_DATA_PTR_OFF) as *const *mut u8);
        for (i, bits) in mapped.into_iter().enumerate() {
            *(data.add((head as usize + i) * 8) as *mut u64) = bits;
        }
        crate::combinator::defer_settle(STATE_FULFILLED, out as i64, 1, REPR_HEAP)
    }
}
