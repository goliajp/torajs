//! §13.15.5.5 IteratorDestructuringAssignmentEvaluation, one element
//! at a time — the ELEMENT half of the park machinery whose rest half
//! lives in [`crate::iter_dstr_rest`].
//!
//! The spec orders each element `lref(i) → step(i) → put(i)`, and
//! `put(i)` before `lref(i+1)`. A whole-pattern walk merges every
//! step into one block, which is only equivalent when no target
//! reference is observable to the iterator. `[ {}[f()] ] = it` is the
//! counter-example: `f()` must run before `next()` is ever looked up.
//! So an assignment pattern with a Member / Index target parks its
//! iterator at a prefix of zero and steps it from here, once per
//! element.
//!
//! Contract: the park argument's stake is TRANSFERRED in (the
//! desugar's slot is overwritten with `out_park` right after, without
//! dropping the old value). The step either hands that stake back
//! through `out_park` or releases it — `out_park` is written on every
//! path, including the throwing one, because §7.4.8 sets [[done]]
//! before the abrupt completion propagates and a done iterator is
//! never closed.

use crate::iter_any::{USER_ITERATOR_LANE, iter_next_inner};
use crate::iter_any_await::AwaitMode;
use crate::iter_dstr_rest::{ParkCursor, ParkLane, is_cursor_cell, park_decode};
use crate::nanbox::{AnyValue, VALUE_UNDEFINED, box_int32};
use crate::nanbox_ffi::__torajs_anyv_rc_dec;

unsafe extern "C" {
    fn __torajs_throw_check() -> i64;
}

/// `__torajs_dstr_step_one(park, recv, out_park)` — take exactly one
/// step of a parked destructuring walk and re-park what is left.
///
/// Answers the element's value (OWNED) and writes the next park into
/// `out_park`:
/// - park `undefined` — the iterator is already done (or a previous
///   step threw). Every further element reads `undefined` and nothing
///   is closed. This is what makes `[a, b, c] = oneElementIterable`
///   work without a length probe: past-end elements simply step a
///   done iterator.
/// - the step throws — §7.4.8 [[done]] = true, so `out_park` stays
///   `undefined` and the pattern's finally close is a no-op (the
///   `*-thrw-close-skip` family asserts exactly this: `next()` threw,
///   `return()` was NOT called).
/// - the step reports done — same, minus the throw: the iterator
///   exhausted itself, so it is not closed (`*-nrml-close-skip`).
/// - the step yields a value — re-park the live cursor, so a pattern
///   that stops short of the iterator's end still owes the close its
///   finally performs (`*-nrml-close`).
///
/// # Safety
/// `park` / `recv` are live AnyValues; `out_park` is a valid slot.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dstr_step_one(
    park: AnyValue,
    recv: AnyValue,
    out_park: *mut AnyValue,
) -> AnyValue {
    unsafe {
        *out_park = VALUE_UNDEFINED;
        if park == VALUE_UNDEFINED {
            return VALUE_UNDEFINED;
        }
        let ParkCursor {
            recv_eff,
            mut idx,
            mut iter_slot,
            lane,
        } = park_decode(park, recv);
        let mut out: AnyValue = VALUE_UNDEFINED;
        let live = iter_next_inner(
            recv_eff,
            &mut idx,
            &mut iter_slot,
            &mut out,
            false,
            AwaitMode::Sync,
        );
        if __torajs_throw_check() != 0 || live == 0 {
            release_park(park, iter_slot, &lane);
            return VALUE_UNDEFINED;
        }
        // Re-park by what the CURSOR now says, not by what the slots
        // hold. `iter_slot` is only an iterator when the step reports
        // the user lane through `idx`: the string lane keeps its
        // FLATTENED receiver there (a per-loop cache), and parking
        // that Str as if it were an iterator made the next step call
        // `next()` on a string. The cursor lanes are read off `lane`
        // rather than by comparing `recv_eff` to `recv`, because a
        // MapIter / ArrIter source parks as ITSELF and the comparison
        // would take it for the indexed lane and leak its stake.
        *out_park = if idx == USER_ITERATOR_LANE {
            iter_slot
        } else if matches!(lane, ParkLane::Cursor) {
            park
        } else if is_cursor_cell(iter_slot) {
            // A Map / Set receiver is not a cursor but derives one on
            // its first step (`map_set_derive_iter`) — that cursor,
            // not the exhausted index, is what resumes.
            iter_slot
        } else {
            // The indexed lane's park is the resume cursor. Anything
            // the step cached beside it cannot ride along, so it is
            // released and rebuilt on the next step — which is why
            // this lane is only taken by patterns that need the spec's
            // interleaving, never by the whole-pattern walk.
            if iter_slot != VALUE_UNDEFINED {
                __torajs_anyv_rc_dec(iter_slot);
            }
            box_int32(idx as i32)
        };
        out
    }
}

/// Settle the transferred park when the walk stops here — an abrupt
/// step or a done report. Releasing is all that is owed: neither
/// completion closes the iterator (§7.4.8 / §7.4.9 both treat a done
/// iterator as already settled).
///
/// # Safety
/// `park` is the live transferred park; `iter_slot` is the cursor's
/// slot after the step.
unsafe fn release_park(park: AnyValue, iter_slot: AnyValue, lane: &ParkLane) {
    unsafe {
        // Two independent stakes, and they never overlap: the CURSOR
        // lane's park is the receiver the step borrowed (its slot
        // stays empty), while the ITERATOR lane's park IS what sits in
        // the slot — released once, below. The indexed lane brought no
        // stake at all, and whatever the step left in the slot (a
        // derived iterator, the string lane's flattened cache) is ours.
        if matches!(lane, ParkLane::Cursor) {
            __torajs_anyv_rc_dec(park);
        }
        if iter_slot != VALUE_UNDEFINED {
            __torajs_anyv_rc_dec(iter_slot);
        }
    }
}
