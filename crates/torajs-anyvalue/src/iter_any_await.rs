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
//!   per §14.7.5.6 step 6.i, and the Promise-or-not probe on the
//!   step result is what tells the two protocols apart.
//!
//! GetIterator runs with hint=async (§7.4.2 step 2.a): the
//! `@@asyncIterator` probe outranks everything; a receiver without
//! one falls to the sync `@@iterator` / builtin lanes above.
//!
//! The Promise unwrap itself is [`__torajs_anyv_await`]'s by-VALUE
//! dispatch — settled value boxed per the cell's repr stamp (+1
//! stake), rejection routed through the throw slot, pending answers
//! undefined under the sync-resolve model. The lower emits a
//! microtask drain before each step call, mirroring the typed await
//! route.

use crate::method_call_promise::__torajs_anyv_await;
use crate::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr, is_cell};
use crate::nanbox_ffi::__torajs_anyv_rc_dec;
use torajs_rc::Tag;

unsafe extern "C" {
    /// torajs-throw — non-zero iff a throw is in flight (a rejected
    /// promise routed through the await hook's throw slot).
    fn __torajs_throw_check() -> i64;
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
/// # Safety
/// `out` is a valid writable pointer holding a live owned AnyValue.
pub(crate) unsafe fn settle_out(out: *mut AnyValue) -> i64 {
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
    unsafe { crate::iter_any::iter_next_inner(recv, idx_slot, iter_slot, out, false, true) }
}
