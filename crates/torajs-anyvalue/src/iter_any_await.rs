//! `__torajs_any_iter_next_await` — the `for await (v of <any>)`
//! drive over the unified runtime iteration protocol (§14.7.5.6
//! iterationKind=async-iterate).
//!
//! Same cascade as [`crate::iter_any`]'s sync entry, plus the two
//! await taps the async drive owes:
//!
//! - **step await** (§14.7.5.6 step 6.d) — a derived iterator whose
//!   `next()` answered a Promise (an async generator object, a user
//!   `@@asyncIterator` iterator) has that Promise awaited before
//!   §7.4.4 reads `done` / `value` off the settled IteratorResult.
//! - **value await** (§27.1.4.4 Async-from-Sync steps 7-9) — a SYNC
//!   lane's value (an indexed element, a MapIter payload, a sync
//!   iterator result's `value`) is awaited before the loop body sees
//!   it: a Promise element unwraps, a rejection forwards. An
//!   async-protocol step skips this tap — its `value` binds verbatim
//!   per §14.7.5.6 step 6.i. The iterator `@@asyncIterator` answered
//!   is the async protocol's ([`AwaitMode::for_lane`]); over a sync
//!   lane a Promise step result stands for it.
//!
//! GetIterator runs with hint=async (§7.4.2 step 2.a): the
//! `@@asyncIterator` probe outranks everything; a receiver without
//! one falls to the sync `@@iterator` / builtin lanes above.
//!
//! The draining drive's unwrap is [`__torajs_anyv_await`]'s by-VALUE
//! dispatch (rejection routed through the throw slot), after a
//! microtask drain the lower emits before each step; the module body
//! awaits in place what a deferred step hands it.
//!
//! `Array.fromAsync` drives the same two taps through the second
//! entry below — it is the one consumer that needs the async
//! GetIterator AND the array-like tail at once.

use crate::anyv_await::__torajs_anyv_await;
use crate::iter_any::{OPEN_THREW_LANE, USER_ASYNC_ITERATOR_LANE};
use crate::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr, is_cell};
use crate::nanbox_ffi::__torajs_anyv_rc_dec;
use core::ffi::c_void;
use torajs_rc::Tag;

unsafe extern "C" {
    /// torajs-throw — non-zero iff a throw is in flight (a rejected
    /// promise routed through the await hook's throw slot).
    fn __torajs_throw_check() -> i64;
    /// torajs-microtask — drain to empty (the sync-resolve model's
    /// await).
    fn __torajs_microtask_run_until_idle();
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    /// torajs-promise `module_await`: the pending throw, one job later
    fn __torajs_module_yield_rethrow();
}

use crate::promise_with_resolvers::{P_STATE_OFF, STATE_PENDING};

/// How a step of the unified protocol takes the awaits `for await`
/// owes (§14.7.5.6 iterationKind async-iterate).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum AwaitMode {
    /// The sync drive: nothing is awaited.
    Sync,
    /// The async drive, settling each await in place by running the
    /// job queue to empty.
    Drain,
    /// The async drive that leaves each await to its caller, the
    /// module body, which awaits in place: the step answers
    /// [`STEP_AWAIT_VALUE`] or [`STEP_AWAIT_RESULT`] with what to
    /// await in `*out`.
    Defer,
    /// [`AwaitMode::Drain`] over an iterator the `@@asyncIterator`
    /// GetIterator resolved: its `next()` answer is awaited
    /// (§14.7.5.6 step 6.d), its `value` never is.
    DrainAsyncIter,
    /// [`AwaitMode::Defer`] over such an iterator: every step answers
    /// [`STEP_AWAIT_RESULT`], a promise or not.
    DeferAsyncIter,
}

impl AwaitMode {
    pub(crate) fn is_async(self) -> bool {
        self != AwaitMode::Sync
    }

    fn defers(self) -> bool {
        matches!(self, AwaitMode::Defer | AwaitMode::DeferAsyncIter)
    }

    /// The step settles a promise `next()` answered in place.
    pub(crate) fn drains(self) -> bool {
        matches!(self, AwaitMode::Drain | AwaitMode::DrainAsyncIter)
    }

    /// A sync lane under the Async-from-Sync iterator: its value is
    /// awaited (§27.1.6.4).
    pub(crate) fn awaits_value(self) -> bool {
        matches!(self, AwaitMode::Drain | AwaitMode::Defer)
    }

    /// The mode a step over the loop's derived iterator takes: an
    /// async drive over the iterator `@@asyncIterator` answered (the
    /// cursor slot holds [`USER_ASYNC_ITERATOR_LANE`]) is the async
    /// protocol's.
    pub(crate) fn for_lane(self, idx: i64) -> AwaitMode {
        match self {
            AwaitMode::Drain if idx == USER_ASYNC_ITERATOR_LANE => AwaitMode::DrainAsyncIter,
            AwaitMode::Defer if idx == USER_ASYNC_ITERATOR_LANE => AwaitMode::DeferAsyncIter,
            m => m,
        }
    }
}

/// A deferred step's `*out` is a sync lane's value, owned: awaiting it
/// gives the loop's value (§27.1.6.4 Async-from-Sync continuation).
pub const STEP_AWAIT_VALUE: i64 = 2;
/// A deferred step's `*out` is what an async iterator's `next()`
/// answered, owned: awaiting it gives the IteratorResult, read by
/// [`__torajs_any_iter_result_step`].
pub const STEP_AWAIT_RESULT: i64 = 3;

/// The step tier's await of what `next()` answered (`*step`, owned):
/// `Ok(was_async)` once it is the IteratorResult to read (settled in
/// place by a draining mode), or `Err(code)` for the step to answer at
/// once: a deferred step hands `*step` over in `*out` (the async
/// protocol's await, which a Promise answer stands for over a sync
/// lane), and a rejection leaves its throw in flight.
///
/// # Safety
/// `*step` is a live owned AnyValue; `out` is writable.
pub(crate) unsafe fn step_result(
    mode: AwaitMode,
    step: &mut AnyValue,
    out: *mut AnyValue,
) -> Result<bool, i64> {
    unsafe {
        let deferred = match mode {
            AwaitMode::DeferAsyncIter => true,
            AwaitMode::Defer => is_promise_cell(*step),
            _ => false,
        };
        if deferred {
            *out = *step;
            return Err(STEP_AWAIT_RESULT);
        }
        let was_async = mode.drains() && await_settle(step);
        if was_async && __torajs_throw_check() != 0 {
            __torajs_anyv_rc_dec(*step);
            *out = VALUE_UNDEFINED;
            return Err(0);
        }
        Ok(was_async)
    }
}

/// Whether the box holds a heap Promise cell.
///
/// # Safety
/// A cell-tagged `av` points at a live heap header.
unsafe fn is_promise_cell(av: AnyValue) -> bool {
    unsafe {
        is_cell(av)
            && (as_void_ptr(av).cast::<u8>().add(4) as *const u16).read() == Tag::Promise as u16
    }
}

/// §27.7.5.1 Await over an owned slot — a heap Promise cell swaps
/// for its settled value (owned; the cell's own stake is released
/// here), everything else stays put. Answers true iff the slot held
/// a Promise — the async-protocol verdict the step tier's value-await
/// gate keys off. A rejection leaves the throw in flight for the
/// caller's check; the swapped-in slot is then `undefined`.
///
/// # Safety
/// `slot` holds a live owned AnyValue.
pub(crate) unsafe fn await_settle(slot: &mut AnyValue) -> bool {
    unsafe {
        if !is_promise_cell(*slot) {
            return false;
        }
        // D4 — an async generator's step promise is settled by the
        // driver's microtasks, which were queued by the `next()` call
        // itself, AFTER the lower's pre-step drain. Under the sync-
        // resolve model a pending cell reads as undefined, so drain
        // once more before the read — the same drain-then-read the
        // typed await route performs.
        if as_void_ptr(*slot).cast::<u8>().add(P_STATE_OFF).read() == STATE_PENDING {
            __torajs_microtask_run_until_idle();
        }
        let settled = __torajs_anyv_await(*slot);
        __torajs_anyv_rc_dec(*slot);
        *slot = settled;
        true
    }
}

/// The §27.1.4.4 value await over a lane's `*out` — answers the
/// lane's live verdict: 1 with the settled value in place, 0 with a
/// rejection's throw in flight and `*out` cleared to undefined (the
/// caller's own throw-check forwards it).
///
/// A deferred step leaves the value to its caller ([`STEP_AWAIT_VALUE`]).
///
/// # Safety
/// `out` is a valid writable pointer holding a live owned AnyValue.
pub(crate) unsafe fn settle_out(out: *mut AnyValue, mode: AwaitMode) -> i64 {
    if mode.defers() {
        return STEP_AWAIT_VALUE;
    }
    unsafe {
        let mut v = *out;
        if await_settle(&mut v) && __torajs_throw_check() != 0 {
            __torajs_anyv_rc_dec(v);
            *out = VALUE_UNDEFINED;
            return 0;
        }
        *out = v;
        1
    }
}

/// See module doc. Same slot contract as `__torajs_any_iter_next`:
/// 1 = `*out` holds an owned AnyValue, 0 = done (or a throw in
/// flight); `iter_slot` starts undefined and belongs to the caller.
///
/// # Safety
/// As `__torajs_any_iter_next`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_iter_next_await(
    recv: AnyValue,
    idx_slot: *mut i64,
    iter_slot: *mut AnyValue,
    out: *mut AnyValue,
) -> i64 {
    unsafe {
        crate::iter_any::iter_next_inner(recv, idx_slot, iter_slot, out, false, AwaitMode::Drain)
    }
}

/// The module body's step of `for await (v of <any>)`: as
/// [`__torajs_any_iter_next_await`], but every await is the caller's
/// (`AwaitMode::Defer`): 0 = done (or a throw in flight), 1 = `*out`
/// holds the value, [`STEP_AWAIT_VALUE`] / [`STEP_AWAIT_RESULT`] =
/// `*out` holds what to await. A sync lane's step that throws (its
/// `next()`, a `done` / `value` read, a result that is no object)
/// rejects the Async-from-Sync wrapper's promise (§27.1.6.2.1
/// IfAbruptRejectPromise, §27.1.6.4 steps 1-4), so the throw is raised
/// a job later; one out of GetIterator is raised at once.
///
/// # Safety
/// As `__torajs_any_iter_next`; the caller is the module body.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_iter_next_defer(
    recv: AnyValue,
    idx_slot: *mut i64,
    iter_slot: *mut AnyValue,
    out: *mut AnyValue,
) -> i64 {
    unsafe {
        let mode = AwaitMode::Defer;
        let code = crate::iter_any::iter_next_inner(recv, idx_slot, iter_slot, out, false, mode);
        let lane = *idx_slot;
        if code == 0
            && lane != USER_ASYNC_ITERATOR_LANE
            && lane != OPEN_THREW_LANE
            && __torajs_throw_check() != 0
        {
            __torajs_module_yield_rethrow();
        }
        code
    }
}

/// The IteratorResult a deferred step's await settled to (`step`,
/// owned, released here), read per §7.4.4 / §7.4.5: 1 with `*out`
/// holding the value, 0 when done or with a throw in flight (a result
/// that is not an object, a throwing `done` / `value` getter).
///
/// # Safety
/// `step` is a live owned AnyValue; `out` is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_iter_result_step(step: AnyValue, out: *mut AnyValue) -> i64 {
    use crate::iter_any_result::iter_result_get;
    unsafe {
        *out = VALUE_UNDEFINED;
        if !is_cell(step)
            || crate::iter_helper_next::is_primitive_heap_cell(as_void_ptr(step) as *const c_void)
        {
            __torajs_anyv_rc_dec(step);
            __torajs_throw_type_error(c"iterator result is not an object".as_ptr());
            return 0;
        }
        let step_ptr = as_void_ptr(step);
        let done = match iter_result_get(step, step_ptr, b"done") {
            None => {
                __torajs_anyv_rc_dec(step);
                return 0;
            }
            Some(v) => {
                let b = crate::nanbox_ffi::__torajs_anyv_to_bool(v);
                __torajs_anyv_rc_dec(v);
                b
            }
        };
        if done {
            __torajs_anyv_rc_dec(step);
            return 0;
        }
        let value = iter_result_get(step, step_ptr, b"value");
        __torajs_anyv_rc_dec(step);
        match value {
            None => 0,
            Some(v) => {
                *out = v;
                1
            }
        }
    }
}
