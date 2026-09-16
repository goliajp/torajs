//! Which entry face a closure cell wants — the one question every
//! site that calls a user callback through a raw fn pointer has to
//! ask before it transmutes.
//!
//! A cell carrying `FLAG_CLOSURE_RECV_FIRST` takes its boxed `__this`
//! between the hidden argc and the first user argument (argc still
//! counts the user arguments only — the S1 spec count). A plain cell
//! has no such word. Transmuting to the wrong one does not fail: the
//! arguments slide by a register and the callback reads its first
//! parameter out of the receiver slot, silently.
//!
//! Every callback face in this crate is spec'd to run with an
//! undefined `this` (§27.2.5.4.1 for then / catch, §27.2.5.3 for
//! finally, and the host callbacks below take no receiver either), so
//! the receiver-first face always gets the undefined box — but it has
//! to get it EXPLICITLY, which is what this module is for.

use core::ffi::c_void;

unsafe extern "C" {
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
}

/// Does this closure cell take a leading boxed receiver?
///
/// # Safety
/// `env` is a live closure cell.
pub(crate) unsafe fn wants_receiver(env: *const c_void) -> bool {
    unsafe {
        let flags = *((env as *const u8).add(6) as *const u16);
        flags & torajs_rc::FLAG_CLOSURE_RECV_FIRST != 0
    }
}

/// The boxed `undefined` a receiver-first face gets for its `this`.
pub(crate) fn undefined_box() -> i64 {
    unsafe { __torajs_anyv_box_from_pair(5, 0) as i64 }
}

/// The callback pointer a closure cell keeps at `env + 8`.
///
/// # Safety
/// `env` is a live closure cell.
pub(crate) unsafe fn fn_ptr(env: *const c_void) -> *mut c_void {
    unsafe { *((env as *const u8).add(8) as *const *mut c_void) }
}
