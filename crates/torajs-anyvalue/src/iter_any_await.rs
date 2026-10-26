//! The `for await (v of <any>)` steps over the unified runtime
//! iteration protocol (§14.7.5.6 iterationKind=async-iterate).
//!
//! Same cascade as [`crate::iter_any`]'s sync entry, plus the two
//! awaits the async drive owes, both left to the caller, which awaits
//! them where it stands (the module body in place, a function's frame
//! by suspending, `iter_any_frame`):
//!
//! - **step await** (§14.7.5.6 step 6.d) — a derived iterator whose
//!   `next()` answered a Promise (an async generator object, a user
//!   `@@asyncIterator` iterator) hands that Promise over
//!   ([`STEP_AWAIT_RESULT`]); [`__torajs_any_iter_result_step`] reads
//!   `done` / `value` off the IteratorResult it settles to.
//! - **value await** (§27.1.4.4 Async-from-Sync steps 7-9) — a SYNC
//!   lane's value (an indexed element, a MapIter payload, a sync
//!   iterator result's `value`) is handed over to be awaited before the
//!   loop body sees it ([`STEP_AWAIT_VALUE`]). An async-protocol step
//!   skips this await — its `value` binds verbatim per §14.7.5.6 step
//!   6.i. The iterator `@@asyncIterator` answered is the async
//!   protocol's ([`AwaitMode::for_lane`]); over a sync lane a Promise
//!   step result stands for it.
//!
//! GetIterator runs with hint=async (§7.4.2 step 2.a): the
//! `@@asyncIterator` probe outranks everything; a receiver without
//! one falls to the sync `@@iterator` / builtin lanes above.
//!
//! `Array.fromAsync` drives the sync protocol (`AwaitMode::Sync`) and
//! awaits in its TypeScript helper.

use crate::iter_any::{OPEN_THREW_LANE, USER_ASYNC_ITERATOR_LANE};
use crate::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr, is_cell};
use crate::nanbox_ffi::__torajs_anyv_rc_dec;
use core::ffi::c_void;
use torajs_rc::Tag;

unsafe extern "C" {
    /// torajs-throw — non-zero iff a throw is in flight.
    fn __torajs_throw_check() -> i64;
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    /// torajs-promise `module_await`: the pending throw, one job later
    fn __torajs_module_yield_rethrow();
}

/// How a step of the unified protocol takes the awaits `for await`
/// owes (§14.7.5.6 iterationKind async-iterate).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum AwaitMode {
    /// The sync drive: nothing is awaited.
    Sync,
    /// The async drive, which leaves each await to its caller: the step
    /// answers [`STEP_AWAIT_VALUE`] or [`STEP_AWAIT_RESULT`] with what
    /// to await in `*out`.
    Defer,
    /// [`AwaitMode::Defer`] over an iterator the `@@asyncIterator`
    /// GetIterator resolved: every step answers [`STEP_AWAIT_RESULT`],
    /// a promise or not (§14.7.5.6 step 6.d), and its `value` is never
    /// awaited.
    DeferAsyncIter,
}

impl AwaitMode {
    pub(crate) fn is_async(self) -> bool {
        self != AwaitMode::Sync
    }

    /// A sync lane under the Async-from-Sync iterator: its value is
    /// awaited (§27.1.6.4).
    pub(crate) fn awaits_value(self) -> bool {
        self == AwaitMode::Defer
    }

    /// The mode a step over the loop's derived iterator takes: an
    /// async drive over the iterator `@@asyncIterator` answered (the
    /// cursor slot holds [`USER_ASYNC_ITERATOR_LANE`]) is the async
    /// protocol's.
    pub(crate) fn for_lane(self, idx: i64) -> AwaitMode {
        match self {
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

/// What `next()` answered (`step`, owned): `Err(code)` hands it over in
/// `*out` for the step to answer at once ([`STEP_AWAIT_RESULT`]: the
/// async protocol's await, which a Promise answer stands for over a
/// sync lane); `Ok(())` leaves it the IteratorResult to read.
///
/// # Safety
/// `step` is a live owned AnyValue; `out` is writable.
pub(crate) unsafe fn step_result(
    mode: AwaitMode,
    step: AnyValue,
    out: *mut AnyValue,
) -> Result<(), i64> {
    unsafe {
        let deferred = match mode {
            AwaitMode::DeferAsyncIter => true,
            AwaitMode::Defer => is_promise_cell(step),
            AwaitMode::Sync => false,
        };
        if deferred {
            *out = step;
            return Err(STEP_AWAIT_RESULT);
        }
        Ok(())
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

/// The module body's step of `for await (v of <any>)`, every await the
/// caller's (`AwaitMode::Defer`), the slot contract of
/// `__torajs_any_iter_next`'s: 0 = done (or a throw in flight), 1 = `*out`
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
