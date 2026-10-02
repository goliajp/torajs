//! §27.2.5.1 `Promise.prototype.catch` and §27.2.5.3
//! `Promise.prototype.finally` as what the spec says they are: Invokes
//! of `then` on the receiver. The builtin kernels
//! (`method_call_promise`) are only the bodies of the builtin `then`;
//! when the receiver's `then` is something else — an own entry in a
//! Promise cell's expando bag, a patched `Promise.prototype.then`, or
//! a receiver that is not a Promise cell at all (the reified
//! `Promise.prototype.catch.call(thenable)`) — the call is that
//! `then`'s, reached through the dispatcher's ordinary GetV, which
//! already answers an own `undefined` / non-callable / poisoned
//! `then` the way §7.3.20 asks.
//!
//! `catch(onRejected)` is `Invoke(this, "then", «undefined, onRejected»)`
//! outright. `finally(onFinally)` mints the thenFinally / catchFinally
//! closures (each a function with `length` 1 and no name, that runs
//! `onFinally`, resolves its result through the receiver's species
//! constructor and re-settles with the original outcome): that is
//! TypeScript the compiler injects (`ast::inject_promise_finally`),
//! registered at startup under [`HELPER_ID_PROMISE_FINALLY`] in the
//! self-hosted bind registry, so the runtime calls the one body the
//! typed lane's patched block and the any lane's arm share.

use core::ffi::c_void;

use torajs_rc::ANY_METHOD_THEN;
use torajs_rc::ns_static::{HELPER_ID_PROMISE_FINALLY, HELPER_ID_PROMISE_FINALLY_RESETTLE};

use crate::method_call_promise::{
    P_PROPS_OFF, P_REPR_OFF, P_STATE_OFF, P_VALUE_OFF, REPR_ANY, STATE_FULFILLED, box_settled,
};
use crate::nanbox::{AnyValue, VALUE_FALSE, VALUE_TRUE, VALUE_UNDEFINED, as_void_ptr, is_cell};

unsafe extern "C" {
    /// torajs-promise — the §27.2.5.3 wait on a returned promise:
    /// its fulfilment forwards the source's settlement into `result`,
    /// its rejection displaces it. Consumes one stake on each cell.
    fn __torajs_promise_finally_wait(source: *mut c_void, inner: *mut c_void, result: *mut c_void);
    fn __torajs_promise_drop(p: *mut c_void);
    fn __torajs_promise_reject(p: *mut c_void, reason: i64);
    fn __torajs_throw_check() -> i64;
    fn __torajs_throw_take() -> i64;
    fn __torajs_throw_take_tag() -> i64;
}

/// A Promise-tagged heap cell.
pub(crate) fn is_promise_cell(av: AnyValue) -> bool {
    if !is_cell(av) {
        return false;
    }
    let p = as_void_ptr(av);
    // SAFETY: a cell box points at a live heap header; the tag word
    // sits at offset 4.
    unsafe { (p.cast::<u8>().add(4) as *const u16).read() == torajs_rc::Tag::Promise as u16 }
}

/// Whether `cell.then` might resolve to something other than the
/// builtin: the cell has an expando bag (an own `then` can only live
/// there — its presence is the ONE load an unpatched cell pays), or
/// `Promise.prototype.then` has a live patch. Existence only — no
/// getter runs here; the Invoke performs the one GET the spec asks.
///
/// # Safety
/// `cell` is a live Promise-tagged heap pointer.
pub(crate) unsafe fn then_is_not_builtin(cell: *mut c_void) -> bool {
    unsafe {
        let props = *(cell.cast::<u8>().add(P_PROPS_OFF) as *const *mut c_void);
        if !props.is_null() {
            return true;
        }
        let recv = crate::nanbox_encode::__torajs_anyv_box_from_pair(4, cell as i64);
        crate::method_call_proto_patch::proto_patch_slot(recv, ANY_METHOD_THEN, core::ptr::null())
            .is_some()
    }
}

/// §27.2.5.1 — `Invoke(this, "then", «undefined, onRejected»)`.
///
/// # Safety
/// `recv` is a live any value; `argv` holds `argc` live slots.
pub(crate) unsafe fn catch_via_then(recv: AnyValue, argv: *const u64, argc: i64) -> AnyValue {
    let on_rejected = if argc >= 1 {
        unsafe { *argv }
    } else {
        VALUE_UNDEFINED
    };
    let args = [VALUE_UNDEFINED, on_rejected];
    unsafe { invoke_then(recv, args.as_ptr(), 2) }
}

/// §27.2.5.3 — the injected helper: species constructor, the two
/// closures, then `Invoke(this, "then", «thenFinally, catchFinally»)`.
/// The registry throws a catchable TypeError when the program never
/// bound it — the injector and this lane key off the same member-name
/// predicate (a computed `p["finally"]` is the recorded boundary,
/// `Array.fromAsync`'s twin).
///
/// # Safety
/// `recv` is a live any value; `argv` holds `argc` live slots.
pub(crate) unsafe fn finally_via_then(recv: AnyValue, argv: *const u64, argc: i64) -> AnyValue {
    let on_finally = if argc >= 1 {
        unsafe { *argv }
    } else {
        VALUE_UNDEFINED
    };
    let args = [recv, on_finally];
    unsafe { crate::method_value::call_bound(HELPER_ID_PROMISE_FINALLY, args.as_ptr(), 2) }
}

/// The Invoke: the dispatcher's throwing entry with the name bytes
/// present (a Proxy receiver's GetV needs them), owned result.
unsafe fn invoke_then(recv: AnyValue, argv: *const u64, argc: i64) -> AnyValue {
    unsafe extern "C" {
        fn __torajs_str_drop(s: *mut c_void);
    }
    unsafe {
        let bytes = b"then";
        let key = crate::__torajs_str_alloc_pooled(bytes.len() as u64);
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), key.add(16), bytes.len());
        let out = crate::method_call::__torajs_any_method_call(
            recv,
            ANY_METHOD_THEN,
            key,
            bytes.len() as i64,
            core::ptr::null_mut(),
            argv,
            argc,
        );
        __torajs_str_drop(key as *mut c_void);
        out
    }
}

/// Staticlib face of the negation of [`then_is_not_builtin`] for the
/// typed `finally` kernel (torajs-promise `then_finally::finish`),
/// which cannot see a cell's expando bag or the prototype patch
/// table from where it lives.
///
/// # Safety
/// `cell` is a live Promise-tagged heap pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_then_is_builtin(cell: *mut c_void) -> i64 {
    unsafe { (!then_is_not_builtin(cell)) as i64 }
}

/// A `finally` handler returned a promise (§27.2.5.3 thenFinally /
/// catchFinally after `promiseResolve`): `inner.then(thunk)` next.
/// With the builtin `then` that is the kernels' direct wait; with
/// any other `then` the Invoke must run — the injected
/// `__promise_finally_resettle(inner, rejected, value)` spells
/// exactly `inner.then(() => value)` / `inner.then(() => { throw
/// reason })`, and what IT answers is waited on the same way (a
/// non-promise answer resolves the result outright). Consumes one
/// stake on each of `source`, `inner`, `result`.
///
/// # Safety
/// All three are live Promise-tagged heap pointers; `source` is
/// settled.
pub(crate) unsafe fn finally_returned_promise(
    source: *mut c_void,
    inner: *mut c_void,
    result: *mut c_void,
) {
    unsafe {
        if !then_is_not_builtin(inner) {
            __torajs_promise_finally_wait(source, inner, result);
            return;
        }
        __torajs_promise_finally_resettle(source, inner, result);
    }
}

/// The non-builtin-`then` half of [`finally_returned_promise`], as
/// the C-ABI face the typed kernel calls too.
///
/// # Safety
/// As [`finally_returned_promise`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_finally_resettle(
    source: *mut c_void,
    inner: *mut c_void,
    result: *mut c_void,
) {
    unsafe {
        let state = source.cast::<u8>().add(P_STATE_OFF).read();
        let repr = source.cast::<u8>().add(P_REPR_OFF).read();
        let value = (source.cast::<u8>().add(P_VALUE_OFF) as *const i64).read();
        // Borrow boxes: the source keeps its value, the inner cell
        // stays this call's until handed on below.
        let args = [
            crate::nanbox_encode::__torajs_anyv_box_from_pair(4, inner as i64),
            if state == STATE_FULFILLED {
                VALUE_FALSE
            } else {
                VALUE_TRUE
            },
            box_settled(repr, value),
        ];
        let answer =
            crate::method_value::call_bound(HELPER_ID_PROMISE_FINALLY_RESETTLE, args.as_ptr(), 3);
        __torajs_promise_drop(inner);
        if __torajs_throw_check() != 0 {
            let ttag = __torajs_throw_take_tag();
            let tval = __torajs_throw_take();
            let reason = crate::nanbox_encode::__torajs_anyv_box_from_pair(ttag, tval);
            crate::method_call_promise::stamp_result(result, REPR_ANY, 1);
            __torajs_promise_reject(result, reason as i64);
            __torajs_promise_drop(source);
            __torajs_promise_drop(result);
            return;
        }
        if is_promise_cell(answer) {
            // `answer`'s stake moves into the wait.
            __torajs_promise_finally_wait(source, as_void_ptr(answer), result);
            return;
        }
        crate::method_call_promise::resolve_result_with(result, answer);
        __torajs_promise_drop(source);
        __torajs_promise_drop(result);
    }
}

/// `__torajs_promise_species_ctor(p)` — §7.3.22 SpeciesConstructor(O,
/// %Promise%) for the injected `finally` helper: `undefined` is the
/// default (`get %Promise%[@@species]` answers %Promise% itself, which
/// the classification recognises as the default), a constructor answers OWNED, a
/// non-object `constructor` or a non-constructor species leaves the
/// TypeError pending and answers undefined. The `constructor` GET
/// runs through the ordinary any member face (own bag, then the
/// chain), and the classification is the buffer family's
/// (`buffer_species`). A kernel rather than TypeScript because a
/// `.constructor` member read in program text is an opaque
/// constructor mention to `builtin_proto_shadow`, which then
/// widens every typed builtin chain in the program to the any lane.
///
/// # Safety
/// `p` is a live any value.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_species_ctor(p: AnyValue) -> AnyValue {
    match unsafe { species_ctor_of(p) } {
        crate::buffer_species::SpeciesResolved::Ctor(c) => c,
        crate::buffer_species::SpeciesResolved::Default
        | crate::buffer_species::SpeciesResolved::Threw => VALUE_UNDEFINED,
    }
}

/// The classification behind [`__torajs_promise_species_ctor`], for
/// the Rust callers that must tell the default from a throw.
///
/// # Safety
/// `p` is a live any value.
pub(crate) unsafe fn species_ctor_of(p: AnyValue) -> crate::buffer_species::SpeciesResolved {
    unsafe extern "C" {
        fn __torajs_str_drop(s: *mut c_void);
    }
    unsafe {
        let bytes = b"constructor";
        let key = crate::__torajs_str_alloc_pooled(bytes.len() as u64);
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), key.add(16), bytes.len());
        let dtag = crate::member_get::__torajs_any_member_get_tag(p, key as *const c_void);
        let dval = crate::member_get_value::__torajs_any_member_get_value(p, key as *const c_void);
        __torajs_str_drop(key as *mut c_void);
        crate::buffer_species::classify_ctor_entry(p, dtag, dval)
    }
}
