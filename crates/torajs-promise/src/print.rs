//! `console.log(promise)` pretty-print — nested-print substrate
//! trunk Commit 8.
//!
//! Emits the bun-minimal Promise form (no value column):
//! - `Promise { <pending> }`
//! - `Promise { <resolved> }`
//! - `Promise { <rejected> }`
//!
//! Bun deliberately doesn't surface the fulfilment value /
//! rejection reason in the default `console.log` form (matches
//! node `util.inspect` default behaviour as of Bun 1.x). The state
//! tag itself is the user-actionable bit.
//!
//! No trailing newline; outer caller appends '\n'.

use core::ffi::c_void;

use crate::layout::{STATE_FULFILLED, STATE_PENDING, STATE_REJECTED};

unsafe extern "C" {
    fn __torajs_io_putc_out(c: i32) -> i32;
}

const PROMISE_STATE_OFF: usize = 8;

#[inline]
unsafe fn put_byte(b: u8) {
    unsafe { __torajs_io_putc_out(b as i32) };
}

#[inline]
unsafe fn put_bytes(s: &[u8]) {
    for &b in s {
        unsafe { put_byte(b) };
    }
}

/// `console.log(p: Promise)` walker — emits the bun state form
/// without a trailing newline.
///
/// # Safety
///
/// `p_ptr` must point to a valid Promise heap block (Tag::Promise,
/// layout per `torajs-promise::layout`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_print(p_ptr: *const c_void) {
    if p_ptr.is_null() {
        unsafe { put_bytes(b"null") };
        return;
    }
    unsafe {
        let state = *((p_ptr as *const u8).add(PROMISE_STATE_OFF));
        match state {
            x if x == STATE_PENDING => put_bytes(b"Promise { <pending> }"),
            x if x == STATE_FULFILLED => put_bytes(b"Promise { <resolved> }"),
            x if x == STATE_REJECTED => put_bytes(b"Promise { <rejected> }"),
            _ => put_bytes(b"Promise { <unknown> }"),
        }
    }
}
