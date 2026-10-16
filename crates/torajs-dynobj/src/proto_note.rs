//! The builtin-prototype change note every kernel that writes,
//! defines or deletes an own entry sends torajs-rc. An ordinary object
//! pays one filter-bit test inside the note; a `<Ctor>.prototype`
//! singleton gets its touched bit raised (and, for a write under a
//! method name, its patch bit), which is how readers learn that the
//! singleton no longer holds what its mint installed.

use core::ffi::c_void;

unsafe extern "C" {
    /// torajs-rc — own-entry change note; the key cell is read only
    /// when `obj` is a singleton, and a null key is a delete.
    fn __torajs_builtin_proto_note_key(obj: *const c_void, key: *const c_void);
}

/// A write or define of `key` on `obj`.
///
/// # Safety
/// `obj` is a live dynobj; `key` is a live Str or Symbol cell.
#[inline(always)]
pub(crate) unsafe fn note_own_write(obj: *mut c_void, key: *const c_void) {
    unsafe { __torajs_builtin_proto_note_key(obj, key) }
}

/// A delete on `obj`.
///
/// # Safety
/// `obj` is any pointer (compared, never dereferenced).
#[inline(always)]
pub(crate) unsafe fn note_delete(obj: *mut c_void) {
    unsafe { __torajs_builtin_proto_note_key(obj, core::ptr::null()) }
}
