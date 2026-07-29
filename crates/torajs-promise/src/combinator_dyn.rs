//! Promise combinators over a dynamically-typed (any-boxed)
//! argument — RFC 20260730-promise-combinator-iterable knife A.
//!
//! ES §27.2.4.{1,2,3,5}: `Promise.all/allSettled/any/race(arg)` runs
//! GetIterator on `arg`; when that throws (the argument is not
//! iterable) the combinator answers a promise REJECTED with that
//! TypeError — it never surfaces as a synchronous error, and under
//! tr's checker it must not stay a whole-program compile reject.
//!
//! Knife A scope: the checker admits only STATICALLY-known
//! non-iterable argument types (Number / Boolean / Null / Undefined
//! / class instances / plain structs) into these entries, so every
//! value arriving here is one GetIterator is defined to throw on —
//! the four entries share the single reject path. `Any` and
//! `String` arguments stay loud compile rejects until knife B lands
//! the runtime tag dispatch (array delegate / string code-point
//! iteration / Symbol.iterator protocol); admitting them against a
//! reject-only kernel would turn spec-iterable values into wrong
//! answers.
//!
//! The TypeError instance is minted through the throw substrate's
//! own composition: `__torajs_throw_type_error` builds the real
//! TypeError (via the registered `__new_TypeError` factory when the
//! program carries one, or the bare-string fallback otherwise) into
//! the throw TLS, and an immediate take_tag + take pops it back out
//! without leaving a pending throw. The pair boxes to an any and
//! settles the rejection cell with `REPR_ANY`, so `.then` /
//! `.catch` receivers decode it like any other any-valued rejection.

use core::ffi::{c_char, c_void};

use crate::layout::REPR_ANY;

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const c_char);
    fn __torajs_throw_take() -> i64;
    fn __torajs_throw_take_tag() -> i64;
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
}

/// Mint a TypeError instance and answer a promise rejected with it.
/// The throw TLS round-trip is the "mint without throwing"
/// composition: take_tag peeks first (take clears `active` but
/// leaves the tag slot), matching the `: any`-typed catch order.
unsafe fn reject_not_iterable() -> *mut c_void {
    unsafe {
        __torajs_throw_type_error(c"value is not iterable".as_ptr());
        let tag = __torajs_throw_take_tag();
        let value = __torajs_throw_take();
        let boxed = __torajs_anyv_box_from_pair(tag, value);
        crate::combinator::defer_settle(crate::layout::STATE_REJECTED, boxed as i64, 1, REPR_ANY)
    }
}

/// # Safety
/// `_v` is an any-boxed value the caller owns; knife A never reads
/// it (every admitted static type is non-iterable by construction).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_all_dyn(_v: u64) -> *mut c_void {
    unsafe { reject_not_iterable() }
}

/// # Safety
/// See [`__torajs_promise_all_dyn`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_race_dyn(_v: u64) -> *mut c_void {
    unsafe { reject_not_iterable() }
}

/// # Safety
/// See [`__torajs_promise_all_dyn`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_any_dyn(_v: u64) -> *mut c_void {
    unsafe { reject_not_iterable() }
}

/// # Safety
/// See [`__torajs_promise_all_dyn`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_allsettled_dyn(_v: u64) -> *mut c_void {
    unsafe { reject_not_iterable() }
}
