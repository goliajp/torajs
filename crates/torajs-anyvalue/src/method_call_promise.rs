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

mod then_any;
use crate::nanbox::{AnyValue, VALUE_NULL, VALUE_UNDEFINED};
use crate::nanbox_encode::{
    __torajs_anyv_box_from_pair, __torajs_anyv_box_pointer, __torajs_anyv_box_str_slot,
};
use then_any::ThenAnyArg;

// ---- torajs-promise layout mirror (lockstep layout.rs) ----
pub(crate) const P_STATE_OFF: usize = 8;
const P_IS_HEAP_OFF: usize = 9;
pub(crate) const P_REPR_OFF: usize = 11;
pub(crate) const P_VALUE_OFF: usize = 16;
/// Lazy expando props dynobj slot (`Promise::props`) — where a
/// `defineProperty(promise, "then", …)` landing lives.
pub(crate) const P_PROPS_OFF: usize = 32;
pub(crate) const STATE_FULFILLED: u8 = 1;
pub(crate) const STATE_REJECTED: u8 = 2;
pub(crate) const REPR_UNSTAMPED: u8 = 0;
const REPR_I64: u8 = 1;
const REPR_F64: u8 = 2;
const REPR_BOOL: u8 = 3;
const REPR_STR: u8 = 4;
const REPR_HEAP: u8 = 5;
pub(crate) const REPR_ANY: u8 = 6;
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
    fn __torajs_promise_drop(p: *mut c_void);
    fn __torajs_throw_check() -> i64;
    fn __torajs_throw_take() -> i64;
    fn __torajs_throw_take_tag() -> i64;
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    fn __torajs_value_drop_heap(p: *mut c_void);
    fn malloc(n: usize) -> *mut c_void;
    fn free(p: *mut c_void);
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
        // §27.2.5.1 / §27.2.5.3 — `catch` and `finally` are Invokes of
        // `then`: a cell whose `then` is not the builtin's (an own
        // entry in its expando bag, or a patched
        // `Promise.prototype.then`) runs THAT `then`, through the
        // dispatcher's ordinary GetV. The sibling holds the bodies;
        // an unpatched cell pays one load and falls through to the
        // kernels below.
        // `finally` reads SpeciesConstructor in its own body (the
        // injected helper), so a cell whose species may differ takes
        // that Invoke lane too.
        if mid != ANY_METHOD_THEN
            && (crate::method_call_promise_invoke::then_is_not_builtin(ptr)
                || (mid == ANY_METHOD_FINALLY
                    && crate::method_call_promise_species::species_may_differ(ptr)))
        {
            let recv = __torajs_anyv_box_from_pair(4, ptr as i64);
            return Some(if mid == ANY_METHOD_CATCH {
                crate::method_call_promise_invoke::catch_via_then(recv, argv, argc)
            } else {
                crate::method_call_promise_invoke::finally_via_then(recv, argv, argc)
            });
        }
        // A user `then` override (a `defineProperty(promise, "then",
        // …)` landing in the expando bag) is an OWN property, and the
        // own-bag gate in `cell_method` (`method_call_bag_expando`)
        // resolves it before this arm is reached — with the receiver
        // as `this`, and honouring `skip_wrapper_expando`. This arm
        // used to consult the bag a second time, without that flag:
        // a bound builtin (`var t = p.then.bind(p); p.then = f; t()`)
        // re-dispatches with the flag set to run the BUILTIN body,
        // and the second consult sent it back into `f` — an infinite
        // recursion (rotation 651, test262 `Promise/any/invoke-then`
        // shape). `catch` / `finally` took the Invoke lane above when
        // `then` is not the builtin's; here they are the builtin's
        // own bodies.
        //
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
        // §27.2.5.4 steps 3-4 — SpeciesConstructor / NewPromiseCapability.
        if mid != ANY_METHOD_FINALLY
            && crate::method_call_promise_species::species_may_differ(ptr)
            && let Some(v) =
                crate::method_call_promise_species::then_via_species(ptr, ok_av, err_av)
        {
            return Some(v);
        }
        // §27.2.5.4 step 3/4 — a non-callable handler slot is EMPTY,
        // and "callable" is §7.2.3. Probing for a closure-cell pair
        // read a Proxy and a class as empty, which does not refuse
        // the program: it forwards the source value as if no handler
        // had been written. The reaction record takes its own stake
        // on the env below and drops it when it runs, so a thunk
        // minted here is owned from that point on.
        let ok_cb = crate::callback_resolve::resolve(ok_av, VALUE_UNDEFINED);
        let err_cb = crate::callback_resolve::resolve(err_av, VALUE_UNDEFINED);
        let (ok_env, ok_entry) = ok_cb
            .as_ref()
            .map_or((core::ptr::null_mut(), 0), |c| (c.env(), c.entry()));
        let (err_env, err_entry) = err_cb
            .as_ref()
            .map_or((core::ptr::null_mut(), 0), |c| (c.env(), c.entry()));
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
        __torajs_rc_inc(result);
        if !ok_env.is_null() {
            __torajs_rc_inc(ok_env);
        }
        if !err_env.is_null() {
            __torajs_rc_inc(err_env);
        }
        then_any::attach(ptr, a);
        Some(__torajs_anyv_box_pointer(result))
    }
}

/// Box the settled slot per the cell's repr stamp. Rc-neutral — the
/// box is a borrow of the cell's stake for the callback's argv slot.
///
/// A typed cell fulfilled with nothing carries `undefined` in its
/// slot's own spelling — the F64 sentinel in a `number` slot, the
/// immortal undefined cell in a pointer slot (`async function f():
/// Promise<number> { if (c) return 1 }` falling off its end). The box
/// reads it back as `undefined`, the way the typed `.value` read does;
/// boxed as a number it printed NaN (643, `async-fallthrough-typed-001`
/// once the awaiter moved to the driver).
pub(crate) unsafe fn box_settled(repr: u8, value: i64) -> AnyValue {
    unsafe {
        match repr {
            REPR_I64 => __torajs_anyv_box_from_pair(2, value),
            REPR_F64 if value as u64 == torajs_rc::F64_UNDEF_SENTINEL_BITS => VALUE_UNDEFINED,
            REPR_F64 => __torajs_anyv_box_from_pair(3, value),
            REPR_BOOL => __torajs_anyv_box_from_pair(1, value),
            REPR_STR => __torajs_anyv_box_str_slot(value as *mut c_void),
            REPR_HEAP if torajs_rc::undef_cell::is_undef_cell(value as *const u8) => {
                VALUE_UNDEFINED
            }
            REPR_HEAP => __torajs_anyv_box_from_pair(4, value),
            REPR_ANY => value as AnyValue,
            REPR_VOID => VALUE_UNDEFINED,
            REPR_NULL => VALUE_NULL,
            // unreachable — the attach-time gate refused UNSTAMPED.
            _ => VALUE_UNDEFINED,
        }
    }
}

/// §27.2.2.1 step 1.h — the handler's return resolves the derived
/// promise through the resolve-function protocol (§27.2.1.3.2): a
/// %Promise% cell is adopted (an attach), a user thenable goes through
/// the NewPromiseResolveThenableJob, an abrupt `then` read rejects,
/// anything else fulfills. `ret` is owned and transfers in.
pub(crate) unsafe fn resolve_result_with(result: *mut c_void, ret: AnyValue) {
    unsafe {
        match crate::promise_thenable::probe_then(ret) {
            crate::promise_thenable::ThenProbe::Threw => {
                let reason = crate::promise_thenable::take_throw_box();
                crate::nanbox_ffi::__torajs_anyv_rc_dec(ret);
                stamp_result(result, REPR_ANY, 1);
                __torajs_promise_reject(result, reason as i64);
            }
            crate::promise_thenable::ThenProbe::Callable(then) => {
                stamp_result(result, REPR_ANY, 1);
                crate::promise_thenable::enqueue_thenable_job(result, ret, then);
            }
            crate::promise_thenable::ThenProbe::NotThenable => {
                if __torajs_promise_adopt_if_thenable(result, REPR_ANY as i64, ret as i64) == 0 {
                    stamp_result(result, REPR_ANY, 1);
                    __torajs_promise_resolve(result, ret as i64);
                }
            }
        }
    }
}

/// Write the result cell's value metadata BEFORE settling it —
/// `resolve`/`reject` drain chained callbacks immediately when the
/// microtask queue is hot, and those read the repr byte.
pub(crate) unsafe fn stamp_result(result: *mut c_void, repr: u8, is_heap: u8) {
    unsafe {
        result.cast::<u8>().add(P_REPR_OFF).write(repr);
        result.cast::<u8>().add(P_IS_HEAP_OFF).write(is_heap);
    }
}
