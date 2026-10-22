//! The sync kernels' own-`then` gate — §27.2.4.1.3 step 6.q-s for
//! an input the typed tier handed over as an array.
//!
//! `Promise.all([p1, p2])` over a `Promise[]` / `any[]` literal takes
//! the `*_sync` kernels: a raw slot walk that attaches to each cell's
//! state directly. That walk never performs the spec's per-element
//! `Get(nextPromise, "then")` + `Call`, so an element wearing its own
//! `then` (`p1.then = function (a, b) { … }`, or a `defineProperty`
//! landing) was silently bypassed — test262's `invoke-then` family
//! counted zero calls where three were due, and the `-error-reject`
//! twins never rejected (measured rotation 651: `cnt 0` on every
//! shape, no output at all for the throwing override).
//!
//! The iterator-interleaved lane (`combinator_iter`, RFC 20260810)
//! already observes `then` per element and invokes an override with
//! a freshly minted resolveElement / rejectElement pair. So the sync
//! kernels ask ONE existence question up front — does any slot's
//! expando bag hold a `then`? — and, when the answer is yes, hand
//! the whole input to that lane as an any-shape array. Inputs whose
//! elements all wear the builtin `then` (every hot shape) pay one
//! null-check per slot and keep their tick-exact walk.
//!
//! The existence probe and the lane's own GET read the same entry
//! under the same predicate (`__torajs_promise_then_overridden` /
//! `__torajs_promise_then_observed`, torajs-anyvalue), so a deferred
//! run always activates the fan-in block and never falls back into
//! the kernel that deferred it.
//!
//! Result shape: the lane answers an `Array<Any>` result for `all`,
//! the same posture the patched-`resolve` detour next door already
//! takes for a typed input (`combinator_patched`). A typed awaiting
//! site over an array whose elements override `then` therefore reads
//! NaN-box slots — registered, not papered over.

use core::ffi::c_void;

use crate::combinator::{arr_len, arr_slot_ptr};

unsafe extern "C" {
    /// torajs-anyvalue — `1` when the cell's expando bag holds a
    /// `then` entry; no GET is performed.
    fn __torajs_promise_then_overridden(cell: *mut c_void) -> i64;
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
    fn __torajs_rc_inc(p: *mut c_void);
    fn __torajs_value_drop_heap(p: *mut c_void);
}

/// `Some(result)` when at least one element overrides `then` — the
/// input was run through the interleaved lane (`mode`'s dyn entry,
/// `sync` its never-activated fallback) and this is its promise.
/// `None` = every element wears the builtin `then`; the caller's own
/// walk proceeds. `arr` is BORROWED throughout.
///
/// # Safety
/// `arr` is a live, non-sparse promises array (typed slots or
/// NaN-box slots, as `arr_is_any` tells).
pub(crate) unsafe fn defer_on_then_override(
    arr: *mut c_void,
    mode: u8,
    sync: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
) -> Option<*mut c_void> {
    unsafe {
        let len = arr_len(arr);
        let is_any = crate::combinator_any::arr_is_any(arr);
        let cell_at = |i: u64| -> Option<*mut c_void> {
            if is_any {
                crate::combinator_any::slot_promise(crate::combinator_any::any_slot(arr, i))
                    .map(|p| p as *mut c_void)
            } else {
                let p = arr_slot_ptr(arr, i);
                (!p.is_null()).then_some(p as *mut c_void)
            }
        };
        let overridden = (0..len).any(|i| cell_at(i).is_some_and(|c| cell_overrides_then(c)));
        overridden.then(|| defer_overridden(arr, mode, sync))
    }
}

/// Does this cell's expando bag hold a `then`? A cell with no bag
/// cannot, and that answer needs no call.
///
/// # Safety
/// `cell` is a live Promise cell.
#[inline(always)]
pub(crate) unsafe fn cell_overrides_then(cell: *mut c_void) -> bool {
    unsafe {
        !(*crate::layout::as_promise(cell)).props.is_null()
            && __torajs_promise_then_overridden(cell) != 0
    }
}

/// Run `arr`, some element of which overrides `then`, through the
/// interleaved lane (see [`defer_on_then_override`]).
///
/// # Safety
/// As [`defer_on_then_override`].
#[cold]
pub(crate) unsafe fn defer_overridden(
    arr: *mut c_void,
    mode: u8,
    sync: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
) -> *mut c_void {
    unsafe {
        let len = arr_len(arr);
        let is_any = crate::combinator_any::arr_is_any(arr);
        // Owned transfer into an any-shape array: each slot takes its
        // own stake (`items_to_any_arr` moves the bits in as owned).
        let mut items: Vec<u64> = Vec::with_capacity(len as usize);
        for i in 0..len {
            let bits = if is_any {
                let b = crate::combinator_any::any_slot(arr, i);
                crate::combinator_any::box_share(b);
                b
            } else {
                let p = arr_slot_ptr(arr, i) as *mut c_void;
                __torajs_rc_inc(p);
                __torajs_anyv_box_from_pair(4, p as i64)
            };
            items.push(bits);
        }
        let any_arr = crate::combinator_dyn::items_to_any_arr(items);
        let boxed = __torajs_anyv_box_from_pair(4, any_arr as i64);
        let out = crate::combinator_dyn::dyn_with_builtin_resolve(mode, boxed, sync);
        __torajs_value_drop_heap(any_arr);
        out
    }
}
