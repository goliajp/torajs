//! Promise-receiver arm of the `any` method dispatcher — `.then` /
//! `.catch` (RFC 20260720-anylane-promise-methods knife 2).
//!
//! The typed tier lowers promise chains against statically-known
//! callback signatures; through an `any` receiver the callback is a
//! boxed-adapter closure and the settled value must be boxed into an
//! AnyValue at dispatch time — which needs the cell's `value_repr`
//! stamp (knife 1). An UNSTAMPED cell (a mint family not yet wired)
//! rejects LOUDLY at attach time, before any callback runs — never a
//! silent mis-box.
//!
//! §27.2.5.4 PerformPromiseThen semantics: a non-callable handler
//! slot is EMPTY (settlement forwards through), `.then(onOk)` runs
//! on FULFILLED and forwards rejection, `.catch(onErr)` is
//! `.then(undefined, onErr)`. A callback throw (pending-throw slot
//! after the boxed invoke) rejects the result promise with the
//! thrown value.
//!
//! Layout / state / repr constants mirror
//! `torajs-promise/src/layout.rs` (independent-layout pattern, must
//! move in lockstep); the attach / settle machinery is reached over
//! the C ABI like every other cross-crate runtime edge.

use core::ffi::c_void;

use torajs_rc::{__torajs_rc_inc, ANY_METHOD_CATCH, ANY_METHOD_FINALLY, ANY_METHOD_THEN};

use crate::method_call_closure_dispatch::{closure_boxed_entry, invoke_boxed};
use crate::nanbox::{AnyValue, VALUE_NULL, VALUE_UNDEFINED};
use crate::nanbox_encode::{
    __torajs_anyv_box_from_pair, __torajs_anyv_box_pointer, __torajs_anyv_box_str_slot,
};

// ---- torajs-promise layout mirror (lockstep layout.rs) ----
const P_STATE_OFF: usize = 8;
const P_IS_HEAP_OFF: usize = 9;
const P_REPR_OFF: usize = 11;
const P_VALUE_OFF: usize = 16;
const STATE_FULFILLED: u8 = 1;
const STATE_REJECTED: u8 = 2;
const REPR_UNSTAMPED: u8 = 0;
const REPR_I64: u8 = 1;
const REPR_F64: u8 = 2;
const REPR_BOOL: u8 = 3;
const REPR_STR: u8 = 4;
const REPR_HEAP: u8 = 5;
const REPR_ANY: u8 = 6;
const REPR_VOID: u8 = 7;
const REPR_NULL: u8 = 8;

unsafe extern "C" {
    fn __torajs_promise_alloc_pending() -> *mut c_void;
    fn __torajs_promise_get_value(p: *const c_void) -> i64;
    fn __torajs_promise_resolve(p: *mut c_void, value: i64);
    fn __torajs_promise_reject(p: *mut c_void, reason: i64);
    /// §27.2.1.3.2 — adopt a promise the handler returned instead of
    /// storing it as the fulfilment value. Non-zero = adopted, and the
    /// returned stake went with it. Implementation (shared with the
    /// typed kernels): torajs-promise `then_adopt.rs`.
    fn __torajs_promise_adopt_if_thenable(result: *mut c_void, repr: i64, value: i64) -> i64;
    fn __torajs_promise_attach_then(
        source: *mut c_void,
        invoke: Option<unsafe extern "C" fn(i64)>,
        arg: i64,
    );
    fn __torajs_promise_drop(p: *mut c_void);
    fn __torajs_throw_check() -> i64;
    fn __torajs_throw_take() -> i64;
    fn __torajs_throw_take_tag() -> i64;
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    fn __torajs_value_drop_heap(p: *mut c_void);
    fn malloc(n: usize) -> *mut c_void;
    fn free(p: *mut c_void);
}

/// Packed attach state for [`then_any_dispatch`]. Every cell /
/// closure ptr in here holds its own +1 stake (taken at attach,
/// released by the dispatcher) — including `result`: the attach
/// site's caller may discard the returned promise before the
/// microtask fires, so the arg keeps it alive. `is_finally`
/// switches the dispatcher to the §27.2.5.3 shape: `on_ok` runs on
/// EITHER settlement, argument-free, return ignored, settlement
/// forwarded (a callback throw still wins as the rejection).
#[repr(C)]
struct ThenAnyArg {
    source: *mut c_void,
    on_ok: *mut c_void,
    on_ok_entry: u64,
    on_err: *mut c_void,
    on_err_entry: u64,
    result: *mut c_void,
    is_finally: i64,
}

/// `.then` / `.catch` on a Promise-tagged cell. `None` = a mid this
/// arm doesn't own (the caller keeps its fall-through). The returned
/// AnyValue is the fresh result promise (owned, boxed-value
/// convention).
pub(crate) unsafe fn promise_method(
    ptr: *mut c_void,
    mid: i64,
    argv: *const u64,
    argc: i64,
) -> Option<AnyValue> {
    if mid != ANY_METHOD_THEN && mid != ANY_METHOD_CATCH && mid != ANY_METHOD_FINALLY {
        return None;
    }
    unsafe {
        // Loud gate FIRST — an unstamped cell means its mint family
        // has no repr wiring yet; refuse at attach time instead of
        // mis-boxing at dispatch time.
        if ptr.cast::<u8>().add(P_REPR_OFF).read() == REPR_UNSTAMPED {
            __torajs_throw_type_error(
                c"promise value form unknown to the any-lane bridge on this receiver".as_ptr(),
            );
            return Some(VALUE_UNDEFINED);
        }
        let a0 = if argc >= 1 { *argv } else { VALUE_UNDEFINED };
        let a1 = if argc >= 2 {
            *argv.add(1)
        } else {
            VALUE_UNDEFINED
        };
        // `finally(cb)` rides the on_ok slot (the dispatcher's
        // is_finally mode runs it on either settlement); its extra
        // args are ignored per §27.2.5.3.
        let (ok_av, err_av) = match mid {
            m if m == ANY_METHOD_CATCH => (VALUE_UNDEFINED, a0),
            m if m == ANY_METHOD_FINALLY => (a0, VALUE_UNDEFINED),
            _ => (a0, a1),
        };
        // §27.2.5.4 step 3/4 — non-callable handler slot is EMPTY.
        let (ok_env, ok_entry) = closure_boxed_entry(ok_av).unwrap_or((core::ptr::null_mut(), 0));
        let (err_env, err_entry) =
            closure_boxed_entry(err_av).unwrap_or((core::ptr::null_mut(), 0));
        let result = __torajs_promise_alloc_pending();
        // Pre-stamp the pending result — a chained `.then` attaches
        // BEFORE this cell settles, and its attach-time gate must
        // see a known form. The dispatcher overwrites with the true
        // settle form (REPR_ANY for a callback run, the forwarded
        // source repr for an empty handler slot).
        stamp_result(result, REPR_ANY, 1);
        let a = malloc(core::mem::size_of::<ThenAnyArg>()) as *mut ThenAnyArg;
        (*a).source = ptr;
        (*a).on_ok = ok_env;
        (*a).on_ok_entry = ok_entry;
        (*a).on_err = err_env;
        (*a).on_err_entry = err_entry;
        (*a).result = result;
        (*a).is_finally = (mid == ANY_METHOD_FINALLY) as i64;
        __torajs_rc_inc(ptr);
        __torajs_rc_inc(result);
        if !ok_env.is_null() {
            __torajs_rc_inc(ok_env);
        }
        if !err_env.is_null() {
            __torajs_rc_inc(err_env);
        }
        __torajs_promise_attach_then(ptr, Some(then_any_dispatch), a as i64);
        Some(__torajs_anyv_box_pointer(result))
    }
}

/// `await <any>` — §27.7.5.1 by-VALUE dispatch for the erased lane
/// (rotation 233). The static tiers dispatch await by TYPE
/// (Promise(T) unwraps, everything else identity); an `any` operand
/// only knows its form at runtime. A heap Promise cell unwraps to
/// its settled value boxed per the cell's repr stamp, +1 stake — the
/// result is an independent binding, mirroring the typed lane's
/// rc_inc after `get_value` (box_settled is a borrow). A REJECTED
/// cell routes through `get_value`'s throw slot (the call site's
/// throw-check propagates); a still-PENDING cell answers undefined
/// (the sync-resolve model's guard, same as the typed read); an
/// UNSTAMPED fulfilled cell refuses loudly — never a silent mis-box.
/// Every other value — including a thenable struct (L3b) — passes
/// through identity WITH a fresh +1: the contract is "+1 stake on
/// every return path", and the lowering call site
/// (`lower_any_await`) releases an owned operand after this call on
/// that assumption. Pre-fix the identity arm returned the same cell
/// without the bump, so `await <owned non-promise cell>` freed the
/// operand's only stake and handed back a dangling pointer (async-gen
/// `yield*` step objects — rotation 288 heisenbug root cause). The
/// call site drains microtasks first, same as the typed await route.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_await(av: AnyValue) -> AnyValue {
    unsafe {
        if !crate::nanbox::is_cell(av) {
            return av;
        }
        let ptr = crate::nanbox::as_void_ptr(av);
        let tag = (ptr.cast::<u8>().add(4) as *const u16).read();
        if tag != torajs_rc::Tag::Promise as u16 {
            crate::nanbox_ffi::__torajs_anyv_rc_inc(av);
            return av;
        }
        let state = ptr.cast::<u8>().add(P_STATE_OFF).read();
        let repr = ptr.cast::<u8>().add(P_REPR_OFF).read();
        let value = __torajs_promise_get_value(ptr);
        if state != STATE_FULFILLED {
            // rejected: get_value set the throw slot; pending: the
            // sync-model undefined guard.
            return VALUE_UNDEFINED;
        }
        if repr == REPR_UNSTAMPED {
            __torajs_throw_type_error(
                c"promise value form unknown to `await` on this receiver".as_ptr(),
            );
            return VALUE_UNDEFINED;
        }
        let out = box_settled(repr, value);
        crate::nanbox_ffi::__torajs_anyv_rc_inc(out);
        out
    }
}

/// Box the settled slot per the cell's repr stamp. Rc-neutral — the
/// box is a borrow of the cell's stake for the callback's argv slot.
unsafe fn box_settled(repr: u8, value: i64) -> AnyValue {
    unsafe {
        match repr {
            REPR_I64 => __torajs_anyv_box_from_pair(2, value),
            REPR_F64 => __torajs_anyv_box_from_pair(3, value),
            REPR_BOOL => __torajs_anyv_box_from_pair(1, value),
            REPR_STR => __torajs_anyv_box_str_slot(value as *mut c_void),
            REPR_HEAP => __torajs_anyv_box_from_pair(4, value),
            REPR_ANY => value as AnyValue,
            REPR_VOID => VALUE_UNDEFINED,
            REPR_NULL => VALUE_NULL,
            // unreachable — the attach-time gate refused UNSTAMPED.
            _ => VALUE_UNDEFINED,
        }
    }
}

/// Write the result cell's value metadata BEFORE settling it —
/// `resolve`/`reject` drain chained callbacks immediately when the
/// microtask queue is hot, and those read the repr byte.
unsafe fn stamp_result(result: *mut c_void, repr: u8, is_heap: u8) {
    unsafe {
        result.cast::<u8>().add(P_REPR_OFF).write(repr);
        result.cast::<u8>().add(P_IS_HEAP_OFF).write(is_heap);
    }
}

/// Handler slot empty — forward the settlement (value shared: one
/// more owner when the slot holds a droppable stake; `rc_inc` is
/// NaN-box aware, so REPR_ANY immediates pass through it unharmed).
unsafe fn forward_settle(result: *mut c_void, rejected: bool, repr: u8, is_heap: u8, value: i64) {
    unsafe {
        if is_heap != 0 && value != 0 {
            __torajs_rc_inc(value as *mut c_void);
        }
        stamp_result(result, repr, is_heap);
        if rejected {
            __torajs_promise_reject(result, value);
        } else {
            __torajs_promise_resolve(result, value);
        }
    }
}

unsafe extern "C" fn then_any_dispatch(arg: i64) {
    let a = arg as *mut ThenAnyArg;
    unsafe {
        let src = (*a).source;
        let state = src.cast::<u8>().add(P_STATE_OFF).read();
        let repr = src.cast::<u8>().add(P_REPR_OFF).read();
        let is_heap = src.cast::<u8>().add(P_IS_HEAP_OFF).read();
        let value = (src.cast::<u8>().add(P_VALUE_OFF) as *const i64).read();
        let rejected = state == STATE_REJECTED;
        let (env, entry) = if rejected {
            ((*a).on_err, (*a).on_err_entry)
        } else {
            ((*a).on_ok, (*a).on_ok_entry)
        };
        let result = (*a).result;
        if (*a).is_finally != 0 {
            // §27.2.5.3 — the callback runs argument-free on either
            // settlement, its return is ignored (thenable-wait
            // matches the typed kernel's posture), the settlement
            // forwards; a callback throw wins as the rejection.
            let cb = (*a).on_ok;
            if !cb.is_null() {
                let _ = invoke_boxed(cb, (*a).on_ok_entry, core::ptr::null(), 0);
            }
            if __torajs_throw_check() != 0 {
                let ttag = __torajs_throw_take_tag();
                let tval = __torajs_throw_take();
                let reason = __torajs_anyv_box_from_pair(ttag, tval);
                stamp_result(result, REPR_ANY, 1);
                __torajs_promise_reject(result, reason as i64);
            } else {
                forward_settle(result, rejected, repr, is_heap, value);
            }
        } else if env.is_null() {
            forward_settle(result, rejected, repr, is_heap, value);
        } else {
            let cb_argv = [box_settled(repr, value)];
            let ret = invoke_boxed(env, entry, cb_argv.as_ptr(), 1);
            if __torajs_throw_check() != 0 {
                // §27.2.2.1 PromiseReactionJob abrupt completion —
                // the callback's throw becomes the result's
                // rejection (take order: tag peek before value).
                let ttag = __torajs_throw_take_tag();
                let tval = __torajs_throw_take();
                let reason = __torajs_anyv_box_from_pair(ttag, tval);
                stamp_result(result, REPR_ANY, 1);
                __torajs_promise_reject(result, reason as i64);
            } else if __torajs_promise_adopt_if_thenable(result, REPR_ANY as i64, ret as i64) == 0 {
                stamp_result(result, REPR_ANY, 1);
                __torajs_promise_resolve(result, ret as i64);
            }
        }
        __torajs_promise_drop(src);
        __torajs_promise_drop(result);
        if !(*a).on_ok.is_null() {
            __torajs_value_drop_heap((*a).on_ok);
        }
        if !(*a).on_err.is_null() {
            __torajs_value_drop_heap((*a).on_err);
        }
        free(a as *mut c_void);
    }
}
