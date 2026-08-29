//! A Promise's settled value, for the trial-deletion walk.
//!
//! `Promise.resolve(o)` takes a reference to `o` and holds it for as
//! long as the promise lives — the drop kernel's
//! `__torajs_value_drop_heap((*pp).value)` is that ownership stated
//! plainly. Rotation 528 gave the tag its property bag; the settled
//! value stayed invisible, so `const p = Promise.resolve(o); o.p = p`
//! was a two-cell ring the walk could not see.
//!
//! Mirror of `torajs-promise/src/layout.rs`:
//!
//! ```text
//! Promise cell (40B)
//! offset | field
//! -------|------
//!   0    | universal heap header (8B)
//!   8    | state (u8) — 0 PENDING / 1 FULFILLED / 2 REJECTED
//!   9    | value_is_heap (u8)
//!  10    | has_handler, value_repr, 4 pad
//!  16    | value (i64) — a cell pointer iff value_is_heap — slot 0
//!  24    | callbacks (*mut PromiseCb)
//!  32    | props (the bag)
//! ```
//!
//! `is_visitable_bag` is what asks whether a Promise has anything to
//! walk — the tag is a bag shape, so its own arm there covers both
//! the bag and the value, and this file needs no second predicate.
//!
//! **The callback list is deliberately not walked.** Each record
//! packs a codegen-emitted dispatcher with an opaque `arg` word — the
//! user closure, the source value and the result promise, encoded by
//! the emit side. The runtime cannot read that word, and the drop
//! kernel does not try either: it frees the record and never follows
//! the `arg`. Reaching a cycle that runs through a pending callback
//! needs a `trace_fn` from the emit side, the way a closure env got
//! one; until then a missed descent under-collects, never corrupts.

use core::ffi::c_void;

/// `Promise::state`; `value_is_heap` is the byte after it.
const PROMISE_STATE_OFF: usize = 8;
/// `Promise::value_is_heap`.
const PROMISE_VALUE_IS_HEAP_OFF: usize = 9;
/// `Promise::value`.
const PROMISE_VALUE_OFF: usize = 16;
/// `STATE_PENDING` — a pending promise holds no value yet, and its
/// `value` word is not a pointer.
const STATE_PENDING: u8 = 0;

/// The child index the settled value is yielded under. A Promise has
/// no entry table, so index 0 is free.
pub const PROMISE_VALUE_SLOT: u64 = 0;

/// The settled value when it is a heap cell, else NULL. The three
/// conditions are the drop kernel's, verbatim: settled, flagged as
/// heap, non-zero.
///
/// # Safety
/// `p` must be a live Promise cell.
#[inline]
pub unsafe fn promise_value(p: *mut c_void) -> *mut c_void {
    let base = p as *const u8;
    if unsafe { *base.add(PROMISE_VALUE_IS_HEAP_OFF) } == 0
        || unsafe { *base.add(PROMISE_STATE_OFF) } == STATE_PENDING
    {
        return core::ptr::null_mut();
    }
    unsafe { *(base.add(PROMISE_VALUE_OFF) as *const *mut c_void) }
}

/// Zero the value slot — `collect_white`'s first-sweep cycle break.
/// The drop kernel's own guard is `value != 0`, so the corpse's
/// teardown skips a slot this collect already accounted for.
///
/// # Safety
/// Same as [`promise_value`].
#[inline]
pub unsafe fn promise_value_clear(p: *mut c_void) {
    unsafe { *((p as *mut u8).add(PROMISE_VALUE_OFF) as *mut u64) = 0 };
}
