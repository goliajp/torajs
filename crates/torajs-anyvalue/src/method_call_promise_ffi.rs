//! What torajs-promise asks torajs-anyvalue across the C ABI.
//!
//! The combinator lanes live in torajs-promise, but three questions
//! they must ask are about a promise cell's EXPANDO BAG and about the
//! resolve-function protocol — both of which live here. Each is a
//! `no_mangle` face over machinery this crate already owns:
//!
//! | face | answers |
//! |---|---|
//! | `__torajs_promise_then_overridden` | is there an own `then`? (no GET) |
//! | `__torajs_promise_then_observed`   | what is it? (the GET, §27.2.4.1.3 step 6.q) |
//! | `__torajs_promise_resolve_protocol`| settle a promise per §27.2.1.3.2 |
//!
//! The first two are the same predicate over the same entry, split by
//! whether an accessor getter runs; the sync kernels ask the cheap one
//! and the interleaved lane performs the observable read.

use core::ffi::c_void;

use crate::method_call_promise::{P_PROPS_OFF, resolve_result_with};
use crate::nanbox::AnyValue;
use crate::nanbox_encode::__torajs_anyv_box_from_pair;

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

/// §27.2.4.{5,6}.1 step j — `race` and `any` hand the element's `then`
/// the RESULT CAPABILITY's own [[Resolve]], not a park-the-value
/// element function (`all` / `allSettled` are the ones that park).
/// So a value that override resolves with goes through the whole of
/// §27.2.1.3.2: a %Promise% cell is adopted, a user thenable rides the
/// NewPromiseResolveThenableJob, an abrupt `then` read rejects.
/// torajs-promise's interleaved lane mints its resolver over the
/// shared fan-in block and used to settle through the raw fulfill
/// kernel, which reads no `then` at all — `Promise.race([p])` with
/// `p.then = r => r(thenable)` answered the thenable object instead of
/// the value it resolves with. `v` is owned and transfers in.
///
/// # Safety
/// `result` is a live pending %Promise% cell; `v` is a live NaN-boxed
/// Any value the caller hands over.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_resolve_protocol(result: *mut c_void, v: u64) {
    if result.is_null() {
        return;
    }
    unsafe { resolve_result_with(result, v as AnyValue) };
}
