//! RFC 20260820-dstr-deferred-close 刀 D — the rest half of a
//! SUSPENDABLE destructuring pattern (`[x, ...t[yield]] = src`).
//!
//! §13.15.5.5 AssignmentRestElement evaluates the rest TARGET's
//! reference first — a yield there suspends — and only then drains
//! the iterator. The bounded walk therefore stops at the pattern's
//! prefix and PARKS "how to resume" in the pattern's `__dstra_it_`
//! slot; after the suspension the rest element drains from the park.
//!
//! The park value is three-way, and the states cannot collide:
//! - a CELL — the still-open derived iterator (user `@@iterator`,
//!   class instance, generator) or a cursor receiver (MapIter /
//!   ArrIter / IterHelper, which step themselves);
//! - an immediate INT — the resume index of a builtin indexed lane
//!   (Arr / Str / StringWrapper behind `any`), which has no iterator
//!   object at all (same immediate-in-an-`any`-slot trick as
//!   `iter_any_array_like`'s parked length);
//! - `undefined` — the walk drained the source to done (a done
//!   iterator is neither closed nor drained further: the rest binds
//!   the empty tail).
//!
//! `__torajs_dstr_close_pending` needs no change for the int state:
//! a non-cell park is nothing to close (`iter_close_value`'s
//! `!is_cell` early-out).

use core::ffi::c_void;

use torajs_rc::Tag;

use crate::iter_any::{
    MethodOutcome, SYM_ITERATOR_METHOD, USER_ITERATOR_LANE, call_obj_method_0, iter_next_inner,
};
use crate::nanbox::{
    AnyValue, VALUE_UNDEFINED, as_int32, as_void_ptr, box_int32, box_void_ptr, is_cell, is_int32,
    is_short_str,
};
use crate::nanbox_ffi::{__torajs_anyv_rc_dec, __torajs_anyv_rc_inc};

unsafe extern "C" {
    fn __torajs_arr_alloc_any(cap: u64) -> *mut u8;
    fn __torajs_arr_push_any(arr: *mut c_void, tag: u64, value: u64) -> *mut u8;
    fn __torajs_throw_check() -> i64;
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

/// `__torajs_dstr_park_pending(recv, iter_slot, idx_slot)` — mint the
/// park value when the bounded walk stops short of done. Owns the
/// answer; the walk stores it into the pattern's `__dstra_it_` slot.
///
/// - a derived-lane walk holds its iterator in `iter_slot`: TAKE that
///   stake (the slot is cleared so the walk's shared release no-ops);
/// - an indexed-lane walk (`iter_slot` still `undefined`, `idx > 0`)
///   parks the resume index as an immediate;
/// - a walk that never stepped (`idx == 0` — the pattern's prefix is
///   empty, `[...t[yield]] = src`) still owes §13.15.5.3 step 1 its
///   GetIterator BEFORE the suspension: derive here, so the iterator
///   both exists for a `gen.return()` close and rejects a
///   non-iterable source at the spec's position.
///
/// One recorded deviation (matches the eager walk's own): a builtin
/// indexable source parks an index, so a `@@iterator` expando
/// installed DURING the suspension is seen by the drain's re-probe
/// where the spec's single GetIterator would not have looked again.
///
/// # Safety
/// `recv` is a live AnyValue; `iter_slot` / `idx_slot` are the walk's
/// valid slots.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dstr_park_pending(
    recv: AnyValue,
    iter_slot: *mut AnyValue,
    idx_slot: *mut i64,
) -> AnyValue {
    unsafe {
        let it = *iter_slot;
        if it != VALUE_UNDEFINED {
            *iter_slot = VALUE_UNDEFINED;
            return it;
        }
        let idx = *idx_slot;
        if idx != 0 {
            return box_int32(idx as i32);
        }
        derive_for_park(recv)
    }
}

/// The never-stepped arm of [`__torajs_dstr_park_pending`] — the same
/// derive question [`crate::iter_any_close`]'s `derive_for_close`
/// asks, but the builtin lanes answer a RESUME INDEX instead of
/// "nothing to close" (the drain has to keep walking them).
///
/// # Safety
/// `recv` is a live AnyValue.
unsafe fn derive_for_park(recv: AnyValue) -> AnyValue {
    unsafe {
        // A user `@@iterator` outranks every builtin lane, exactly as
        // the stepping path orders it.
        match crate::iter_any_get_method::get_iterator(recv) {
            crate::iter_any_get_method::GetIterator::Iterator(iter) => return iter,
            crate::iter_any_get_method::GetIterator::Threw => return VALUE_UNDEFINED,
            crate::iter_any_get_method::GetIterator::NoUserMethod => {}
        }
        if is_short_str(recv) {
            return box_int32(0);
        }
        if !is_cell(recv) {
            __torajs_throw_type_error(c"value is not iterable".as_ptr());
            return VALUE_UNDEFINED;
        }
        let tag = (as_void_ptr(recv).cast::<u8>().add(4) as *const u16).read();
        if tag == Tag::Str as u16
            || tag == Tag::Arr as u16
            || tag == Tag::StringWrapper as u16
            || tag == Tag::Map as u16
            || tag == Tag::Set as u16
        {
            // Indexed / derivable builtin — the drain's step derives
            // (or indexes) from the receiver itself.
            return box_int32(0);
        }
        if tag == Tag::MapIter as u16 || tag == Tag::ArrIter as u16 || tag == Tag::IterHelper as u16
        {
            // The receiver IS the cursor — park it (retained; the
            // slot's own drop settles the stake).
            __torajs_anyv_rc_inc(recv);
            return recv;
        }
        if tag != Tag::Obj as u16 {
            __torajs_throw_type_error(c"value is not iterable".as_ptr());
            return VALUE_UNDEFINED;
        }
        match call_obj_method_0(as_void_ptr(recv) as *mut c_void, SYM_ITERATOR_METHOD) {
            MethodOutcome::Ok(iter) => iter,
            MethodOutcome::Missing => {
                __torajs_throw_type_error(c"value is not iterable".as_ptr());
                VALUE_UNDEFINED
            }
            MethodOutcome::Threw => VALUE_UNDEFINED,
        }
    }
}

/// `__torajs_dstr_drain_rest(park, recv)` — §13.15.5.5 step 4: drain
/// what the park left open into a fresh dense `Array<Any>` (owned,
/// boxed). Runs AFTER the rest target's reference evaluated (and its
/// yield resumed); the desugar clears the park slot before this call,
/// which is §7.4.8's [[done]] = true — an abrupt step below must NOT
/// re-close the iterator on the way out (the thrw-close-skip family).
///
/// Both arguments are borrows; a mid-drain throw releases the partial
/// array and forwards the pending throw (the caller's throw-check
/// unwinds).
///
/// # Safety
/// `park` / `recv` are live AnyValues (or `undefined`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dstr_drain_rest(park: AnyValue, recv: AnyValue) -> AnyValue {
    unsafe {
        let mut arr = __torajs_arr_alloc_any(0);
        if park == VALUE_UNDEFINED {
            // Drained-to-done park — the rest is the empty tail.
            return box_void_ptr(arr as *mut c_void);
        }
        let mut idx: i64;
        let mut iter_slot: AnyValue = VALUE_UNDEFINED;
        let mut recv_eff = recv;
        // `true` when `iter_slot` holds the BORROWED park — the loop
        // must not release it (the pattern's slot copy owns it).
        let mut borrowed_iter = false;
        if is_int32(park) {
            idx = as_int32(park) as i64;
        } else {
            let tag = (as_void_ptr(park).cast::<u8>().add(4) as *const u16).read();
            if tag == Tag::MapIter as u16
                || tag == Tag::ArrIter as u16
                || tag == Tag::IterHelper as u16
            {
                // Cursor receivers step themselves; a non-zero index
                // skips the first-step GetIterator probe.
                recv_eff = park;
                idx = 1;
            } else {
                iter_slot = park;
                idx = USER_ITERATOR_LANE;
                borrowed_iter = true;
            }
        }
        let mut out: AnyValue = VALUE_UNDEFINED;
        loop {
            let live = iter_next_inner(recv_eff, &mut idx, &mut iter_slot, &mut out, false, false);
            if __torajs_throw_check() != 0 {
                __torajs_anyv_rc_dec(box_void_ptr(arr as *mut c_void));
                if !borrowed_iter {
                    __torajs_anyv_rc_dec(iter_slot);
                }
                return VALUE_UNDEFINED;
            }
            if live == 0 {
                break;
            }
            let t = crate::__torajs_anyv_unbox_tag(out);
            let p = crate::__torajs_anyv_unbox_value(out);
            arr = __torajs_arr_push_any(arr as *mut c_void, t as u64, p as u64);
        }
        if !borrowed_iter {
            __torajs_anyv_rc_dec(iter_slot);
        }
        box_void_ptr(arr as *mut c_void)
    }
}
