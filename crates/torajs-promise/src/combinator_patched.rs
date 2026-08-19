//! §27.2.4 combinator lane over a PATCHED `Promise.resolve` —
//! rotation 448, the kernel half of the static-slot patch protocol.
//!
//! Every combinator's spec text runs GetPromiseResolve(C) and then
//! `Call(promiseResolve, C, «nextValue»)` per element (§27.2.4.1.3
//! step 6.i and its all/allSettled/any/race siblings). The typed
//! fast paths below never perform that call — sound only while the
//! `resolve` slot holds the builtin. Each `*_sync` entry therefore
//! gates on [`consult_active`] (two loads and a compare when
//! unpatched, torajs-anyvalue's probe over the ctor cell's expando
//! dict) and detours here when a user override is installed.
//!
//! The detour is one collect-then-delegate, the `combinator_dyn`
//! shape: run the patched `resolve` over every element (this = the
//! ctor cell, per the spec's `C`), collect the answered promises
//! into an any-shape array, and hand it to the matching `*_sync_any`
//! sibling — NOT back to the `no_mangle` entry, which would probe
//! again and recurse. Element calls run before any `then` attach
//! (the spec interleaves attach per element; the t262 face asserts
//! call count / argument / this, which this ordering preserves).
//!
//! Abrupt completions follow §27.2.4.1 step 7 IfAbruptRejectPromise:
//! a throwing override answers a promise REJECTED with the thrown
//! value ([`crate::combinator_dyn::reject_with_pending_throw`]). An
//! override answering a non-promise is the one place the typed lane
//! cannot follow the spec's arbitrary-thenable road — it rejects
//! with a loud TypeError instead (recorded divergence, plan-state
//! L3b).

use core::ffi::c_void;

use crate::combinator::{arr_len, arr_slot_ptr};

unsafe extern "C" {
    /// torajs-anyvalue — the patch probe + the per-element patched
    /// invocation (owned Any answer, throw left pending).
    fn __torajs_promise_ctor_patched(name_id: i64) -> i64;
    fn __torajs_promise_call_patched(name_id: i64, arg: u64) -> u64;
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
    fn __torajs_anyv_rc_dec(v: u64);
    fn __torajs_throw_check() -> i64;
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    fn __torajs_value_drop_heap(p: *mut c_void);
}

/// Whether `Promise.resolve` carries a user patch — the combinator
/// entries' gate.
pub(crate) unsafe fn consult_active() -> bool {
    unsafe { __torajs_promise_ctor_patched(0) != 0 }
}

/// `Call(promiseResolve, C, «v»)` over every element; the collected
/// answers as an owned any-shape array, or the rejected promise an
/// abrupt element produced.
unsafe fn resolve_elements(arr: *mut c_void) -> Result<*mut c_void, *mut c_void> {
    unsafe {
        let len = arr_len(arr);
        let is_any = crate::combinator_any::arr_is_any(arr);
        let mut items: Vec<u64> = Vec::with_capacity(len as usize);
        for i in 0..len {
            // The element rides as a BORROWED box — an any-slot input
            // hands its bits over verbatim, a typed slot's promise
            // pointer heap-boxes (pure encode).
            let v = if is_any {
                crate::combinator_any::any_slot(arr, i)
            } else {
                __torajs_anyv_box_from_pair(4, arr_slot_ptr(arr, i) as i64)
            };
            let ret = __torajs_promise_call_patched(0, v);
            if __torajs_throw_check() == 0 && crate::combinator_any::slot_promise(ret).is_none() {
                // The typed lane's return contract — see module doc.
                __torajs_anyv_rc_dec(ret);
                __torajs_throw_type_error(
                    c"the patched Promise.resolve answered a non-promise into a typed slot"
                        .as_ptr(),
                );
            }
            if __torajs_throw_check() != 0 {
                for it in items {
                    __torajs_anyv_rc_dec(it);
                }
                return Err(crate::combinator_dyn::reject_with_pending_throw());
            }
            items.push(ret);
        }
        Ok(crate::combinator_dyn::items_to_any_arr(items))
    }
}

/// Collect-then-delegate over one any-lane sibling; the temp array's
/// stake releases after the sibling incs what it keeps (the
/// `combinator_iter` delegate shape).
unsafe fn run(arr: *mut c_void, sibling: fn(*mut c_void) -> *mut c_void) -> *mut c_void {
    unsafe {
        match resolve_elements(arr) {
            Err(rejected) => rejected,
            Ok(new_arr) => {
                let out = sibling(new_arr);
                __torajs_value_drop_heap(new_arr);
                out
            }
        }
    }
}

pub(crate) unsafe fn run_all(arr: *mut c_void) -> *mut c_void {
    unsafe { run(arr, |a| crate::combinator_any::all_sync_any(a)) }
}

pub(crate) unsafe fn run_race(arr: *mut c_void) -> *mut c_void {
    unsafe { run(arr, |a| crate::combinator_any::race_sync_any(a)) }
}

pub(crate) unsafe fn run_any(arr: *mut c_void) -> *mut c_void {
    unsafe { run(arr, |a| crate::combinator_any::any_sync_any(a)) }
}

pub(crate) unsafe fn run_allsettled(arr: *mut c_void, record_tags: u64) -> *mut c_void {
    unsafe {
        match resolve_elements(arr) {
            Err(rejected) => rejected,
            Ok(new_arr) => {
                let out = crate::combinator_any::allsettled_sync_any(new_arr, record_tags);
                __torajs_value_drop_heap(new_arr);
                out
            }
        }
    }
}
