//! Promise<T> substrate for the torajs AOT TypeScript runtime.
//!
//! Layer-3 substrate of the architecture rewrite (P6.1, 2026-05-24).
//! Replaces the bulk of `runtime_promise.c` — the entire Promise
//! surface (alloc + pool + drop + resolve/reject + .then/.catch/
//! .finally + .all/.allSettled/.race/.any + queueMicrotask) now
//! lives in this pure-Rust crate. Microtask queue itself lives in
//! the sibling `torajs-microtask` crate (P5); this crate's resolve/
//! reject/attach_then paths call into it via `extern "C"`.
//!
//! ## Module split (each ≤ 500 LOC HARD RULE)
//!
//! - [`layout`] — Promise + PromiseCb struct + constants + heap
//!   header + MicrotaskFn typedef. ABI-shared with the ssa_lower-
//!   emitted dispatcher fns + every C-side caller's struct read.
//! - [`pool`] — bounded free-list pool + 5 alloc variants
//!   (pending/fulfilled/rejected × primitive/heap) + drop +
//!   thenable absorption.
//! - [`state`] — resolve/reject + drain_callbacks + get_value/
//!   get_state + attach_then. Hot path: drain enqueues each cb
//!   onto the microtask queue.
//! - [`then`] — .then/.catch/.finally (6 variants: simple +
//!   closure × 3 handler kinds). Each variant allocs a small heap
//!   arg struct + a dispatcher, then `attach_then`s the dispatcher.
//! - [`combinator`] — .all/.allSettled/.race/.any sync combinators.
//!   MVP fast path: all-already-settled inputs build result Array;
//!   pending input → reject with placeholder reason (real fan-in
//!   post-T-15.g.6).
//! - [`micro`] — queueMicrotask globals (closure + named-fn
//!   variants). Both pack cb through the queue's i64 arg slot.

// v0.7-A2 step 6b — force-link mmalloc.
extern crate torajs_mmalloc as _;

pub mod combinator;
pub mod layout;
pub mod micro;
pub mod pool;
pub mod state;
pub mod then;

pub use combinator::{
    __torajs_promise_all_sync, __torajs_promise_allsettled_sync, __torajs_promise_any_sync,
    __torajs_promise_race_sync,
};
pub use micro::{__torajs_queue_microtask_closure, __torajs_queue_microtask_simple};
pub use pool::{
    __torajs_promise_alloc_fulfilled, __torajs_promise_alloc_fulfilled_heap,
    __torajs_promise_alloc_pending, __torajs_promise_alloc_rejected,
    __torajs_promise_alloc_rejected_heap, __torajs_promise_drop, __torajs_promise_resolve_thenable,
};
pub use state::{
    __torajs_promise_attach_then, __torajs_promise_get_state, __torajs_promise_get_value,
    __torajs_promise_reject, __torajs_promise_resolve,
};
pub use then::{
    __torajs_promise_catch_closure, __torajs_promise_catch_simple, __torajs_promise_finally,
    __torajs_promise_finally_closure, __torajs_promise_then_closure, __torajs_promise_then_simple,
};

// Cross-tier extern stubs for cargo unit tests — real symbols
// live in libs (torajs-rc, torajs-throw, libtorajs_microtask) +
// C runtime files at `tr build` link time. cargo test doesn't link
// any of those, so panicking stubs keep the test binary linking
// clean. Same pattern as torajs-collections / torajs-weak /
// torajs-cycle test stubs.
#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_rc_inc(_p: *mut core::ffi::c_void) {
    panic!("torajs-promise test stub: rc_inc should not be called from cargo test");
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_rc_dec(_p: *mut core::ffi::c_void) -> i32 {
    panic!("torajs-promise test stub: rc_dec should not be called from cargo test");
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_value_drop_heap(_p: *mut core::ffi::c_void) {
    panic!("torajs-promise test stub: value_drop_heap should not be called from cargo test");
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_microtask_enqueue(_fn_: layout::MicrotaskFn, _arg: i64) {
    panic!("torajs-promise test stub: microtask_enqueue should not be called from cargo test");
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_throw_set(_tag: i64, _value: i64) {
    panic!("torajs-promise test stub: throw_set should not be called from cargo test");
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_alloc(_cap: u64) -> *mut core::ffi::c_void {
    panic!("torajs-promise test stub: arr_alloc should not be called from cargo test");
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_push(
    _arr: *mut core::ffi::c_void,
    _val: i64,
) -> *mut core::ffi::c_void {
    panic!("torajs-promise test stub: arr_push should not be called from cargo test");
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_alloc_pooled(_len: u64) -> *mut u8 {
    panic!("torajs-promise test stub: str_alloc_pooled should not be called from cargo test");
}
