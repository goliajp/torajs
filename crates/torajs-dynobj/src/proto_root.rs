//! The %Object.prototype% singleton's mark, and the one place a new
//! entry joins a dict.
//!
//! A struct cell that lacks a declared member (an object literal left
//! it out, `delete` took it) and carries no user [[Prototype]] inherits
//! from %Object.prototype% alone, so `"a" in o` on it is the question
//! "does %Object.prototype% own `a`". The typed `in` answers it inline
//! off torajs-rc's `__torajs_object_proto_touched` while the flag is
//! down; every insert into the singleton raises it, through
//! [`note_entry_added`], which the five insert sites call instead of
//! bumping the two counters by hand.

use core::ffi::c_void;

use crate::layout::DYNOBJ_HDR_FLAG_OBJECT_PROTO;
use crate::probe::{count, set_count, set_entries_len};

unsafe extern "C" {
    /// torajs-rc — raises `__torajs_object_proto_touched`.
    fn __torajs_object_proto_touch();
}

/// Mark `obj` as the %Object.prototype% singleton. Called by the mint
/// once the `__proto__` accessor entry is in, so that entry does not
/// count as user code touching the prototype.
///
/// # Safety
/// `obj` is null (no-op) or a live dynobj heap pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_mark_object_proto(obj: *mut c_void) {
    if obj.is_null() {
        return;
    }
    unsafe {
        let flags = (obj as *mut u8).add(6) as *mut u16;
        *flags |= DYNOBJ_HDR_FLAG_OBJECT_PROTO;
    }
}

/// The entry at dense index `e_idx` has just been written: the dense
/// length and the live count move past it, and the singleton's flag
/// goes up when `obj` is %Object.prototype%.
///
/// # Safety
/// `obj` is a live dynobj heap pointer whose entry `e_idx` is written.
#[inline]
pub(crate) unsafe fn note_entry_added(obj: *mut c_void, e_idx: u32) {
    unsafe {
        set_entries_len(obj, e_idx + 1);
        set_count(obj, count(obj) + 1);
        let flags = *((obj as *const u8).add(6) as *const u16);
        if flags & DYNOBJ_HDR_FLAG_OBJECT_PROTO != 0 {
            __torajs_object_proto_touch();
        }
    }
}
