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
pub(crate) const P_STATE_OFF: usize = 8;
const P_IS_HEAP_OFF: usize = 9;
pub(crate) const P_REPR_OFF: usize = 11;
pub(crate) const P_VALUE_OFF: usize = 16;
/// Lazy expando props dynobj slot (`Promise::props`) — where a
/// `defineProperty(promise, "then", …)` landing lives.
pub(crate) const P_PROPS_OFF: usize = 32;
pub(crate) const STATE_FULFILLED: u8 = 1;
const STATE_REJECTED: u8 = 2;
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
        // §27.2.5.1 / §27.2.5.3 — `catch` and `finally` are Invokes of
        // `then`: a cell whose `then` is not the builtin's (an own
        // entry in its expando bag, or a patched
        // `Promise.prototype.then`) runs THAT `then`, through the
        // dispatcher's ordinary GetV. The sibling holds the bodies;
        // an unpatched cell pays one load and falls through to the
        // kernels below.
        if mid != ANY_METHOD_THEN && crate::method_call_promise_invoke::then_is_not_builtin(ptr) {
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

/// `__torajs_promise_then_observed(cell)` — §27.2.4.1.3 step 6.q's
/// per-element `then` GET over a promise cell's expando bag (a
/// `defineProperty(promise, "then", …)` landing at `Promise::props`).
/// `0` = no override, take the builtin / cell-state path. Anything
/// else is the GET result, OWNED (+1): a data entry's value, or an
/// accessor getter's answer — whose throw stays pending with
/// `undefined` returned, the [`crate::len_get::box_probe_pair`]
/// contract, so the caller's throw check separates "getter threw"
/// from "then is honestly undefined" (the latter proceeds to the
/// not-callable TypeError §27.2.4.1.3 step 6.r would raise).
///
/// # Safety
/// `cell` is a live Promise-tagged heap pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_then_observed(cell: *mut c_void) -> u64 {
    unsafe extern "C" {
        fn __torajs_str_drop(s: *mut c_void);
    }
    unsafe {
        let Some((tag, value, key)) = own_then_entry(cell) else {
            return 0;
        };
        // Borrow-boxed receiver for the getter's `this` — the cell
        // stays the caller's.
        let recv = __torajs_anyv_box_from_pair(4, cell as i64);
        let v = crate::len_get::box_probe_pair(tag, value, recv, key);
        __torajs_str_drop(key);
        v
    }
}

/// `__torajs_promise_then_overridden(cell)` — the EXISTENCE half of
/// [`__torajs_promise_then_observed`]: `1` when the cell's expando
/// bag holds a `then` entry, `0` otherwise, with no GET performed
/// (an accessor's getter is not run — that observable read belongs
/// to the Invoke lane). The sync combinator kernels ask this before
/// their raw slot walk: an input array whose elements all wear the
/// builtin `then` keeps its tick-exact walk, and one element with
/// an override sends the whole array down the iterator-interleaved
/// lane, which is where §27.2.4.1.3 step 6.q-s already lives. The
/// two answers are the same predicate over the same entry, so the
/// lane the sync kernel defers to always activates — the deferral
/// cannot bounce back.
///
/// # Safety
/// `cell` is a live Promise-tagged heap pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_then_overridden(cell: *mut c_void) -> i64 {
    unsafe extern "C" {
        fn __torajs_str_drop(s: *mut c_void);
    }
    unsafe {
        match own_then_entry(cell) {
            Some((_, _, key)) => {
                __torajs_str_drop(key);
                1
            }
            None => 0,
        }
    }
}

/// The `then` entry of a promise cell's expando bag: `(tag, value,
/// key)` with `key` OWNED (the caller drops it), or `None` when the
/// cell has no bag or the bag has no `then`. A data entry holding
/// `undefined` reads as absent — its tag IS the absent sentinel —
/// which both callers above treat as "no override".
unsafe fn own_then_entry(cell: *mut c_void) -> Option<(u64, u64, *mut c_void)> {
    unsafe extern "C" {
        fn __torajs_dynobj_get_tag(obj: *const c_void, key: *const c_void) -> u64;
        fn __torajs_dynobj_get_value(obj: *const c_void, key: *const c_void) -> u64;
        fn __torajs_str_drop(s: *mut c_void);
    }
    /// dynobj absent sentinel (torajs-dynobj layout mirror).
    const ANY_UNDEF: u64 = 5;
    unsafe {
        let props = *(cell.cast::<u8>().add(P_PROPS_OFF) as *const *mut c_void);
        if props.is_null() {
            return None;
        }
        let key = {
            let bytes = b"then";
            let s = crate::__torajs_str_alloc_pooled(bytes.len() as u64);
            core::ptr::copy_nonoverlapping(bytes.as_ptr(), s.add(16), bytes.len());
            s as *mut c_void
        };
        let tag = __torajs_dynobj_get_tag(props, key);
        let value = __torajs_dynobj_get_value(props, key);
        if tag == ANY_UNDEF {
            __torajs_str_drop(key);
            return None;
        }
        Some((tag, value, key))
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
        let mut handed_off = false;
        if (*a).is_finally != 0 {
            // §27.2.5.3 — the callback runs argument-free on either
            // settlement; a throw wins as the rejection; a returned
            // promise is waited on (`promiseResolve(C, r).then(thunk)`
            // — its rejection displaces the settlement, its fulfilment
            // forwards the source's), and a returned promise whose
            // `then` is not the builtin's is Invoked, closures and
            // all; any other return is dropped and the settlement
            // forwards. The wait hands the source / result stakes to
            // the job, so the tail's drops are skipped for it.
            let cb = (*a).on_ok;
            let ret = if cb.is_null() {
                VALUE_UNDEFINED
            } else {
                invoke_boxed(cb, (*a).on_ok_entry, core::ptr::null(), 0)
            };
            if __torajs_throw_check() != 0 {
                let ttag = __torajs_throw_take_tag();
                let tval = __torajs_throw_take();
                let reason = __torajs_anyv_box_from_pair(ttag, tval);
                stamp_result(result, REPR_ANY, 1);
                __torajs_promise_reject(result, reason as i64);
            } else if crate::method_call_promise_invoke::is_promise_cell(ret) {
                crate::method_call_promise_invoke::finally_returned_promise(
                    src,
                    crate::nanbox::as_void_ptr(ret) as *mut c_void,
                    result,
                );
                handed_off = true;
            } else {
                crate::nanbox_ffi::__torajs_anyv_rc_dec(ret);
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
            } else {
                resolve_result_with(result, ret);
            }
        }
        if !handed_off {
            __torajs_promise_drop(src);
            __torajs_promise_drop(result);
        }
        if !(*a).on_ok.is_null() {
            __torajs_value_drop_heap((*a).on_ok);
        }
        if !(*a).on_err.is_null() {
            __torajs_value_drop_heap((*a).on_err);
        }
        free(a as *mut c_void);
    }
}
