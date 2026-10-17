//! The `Tag::Obj` (class-instance) arm of [`crate::prop_delete`] —
//! §10.1.10 [[Delete]] over a struct cell. Split out under the
//! 500-line file rule when RFC 20260915-declared-field-delete added
//! the deleted-field record; the parent keeps the receiver table and
//! the shared refusal helpers this arm borrows.

use core::ffi::c_void;

use crate::prop_delete::{BUCKET_FLAG_CONFIGURABLE, refuse, refuse_non_configurable};

unsafe extern "C" {
    /// torajs-dynobj — the live attributes of a DECLARED class member
    /// (its redefine sidecar when one exists, else the layout default
    /// the integrity level moves), -1 when the layout declares nothing
    /// under the key, -2 for an accessor an ordinary property replaced.
    /// See that fn's doc for why the question is asked rather than
    /// restated here.
    fn __torajs_obj_declared_field_attrs(obj: *mut c_void, key: *mut c_void) -> i64;
    /// torajs-dynobj — record a declared field's deletion in its
    /// sidecar (RFC 20260915-declared-field-delete).
    fn __torajs_obj_declared_field_delete(obj: *mut c_void, key: *mut c_void) -> i64;
    fn __torajs_dynobj_delete(obj: *mut c_void, key: *const c_void) -> i32;
}

/// §10.1.10 over a class-instance cell — the `Tag::Obj` arm.
///
/// A struct cell carries own properties in two places, and only one of
/// them can lose an entry. The `+24` expando dict is an ordinary
/// dynobj, so a key it owns deletes exactly as it does on every other
/// receiver — including the step-4 refusal a `defineProperty`'d entry
/// can arm. A DECLARED member is a fixed layout slot with nowhere to
/// go, so it keeps the recorded divergence (bun deletes and answers
/// true; we answer false) — but "not removable" and "not
/// configurable" are different sentences, and only the second one
/// throws. Asking for the attributes tells us which we are in:
/// a frozen or sealed instance, or one whose field was redefined
/// `{configurable: false}`, refuses with the strict TypeError that
/// test262's `verifyNotConfigurable` probes for.
///
/// The one declared member that CAN detach is the error `message`
/// line (§20.5.6.1.1): its slot holds an own-absence sentinel, so the
/// delete swaps rather than removes. It reaches that swap through the
/// same configurable gate as everything else.
///
/// # Safety
/// `ptr` is a live `Tag::Obj` heap pointer; `key` is a live Str cell
/// (symbol keys are routed by the caller before this arm).
pub(crate) unsafe fn struct_delete(
    ptr: *mut c_void,
    key: *const c_void,
    throw_on_refusal: bool,
) -> i64 {
    // §13.5.1.2 — deleting a property that is not there succeeds; a
    // deleted declared field's record clears the entry's attribute
    // bits, so this must come before the configurable gate.
    if unsafe { crate::struct_probe::declared_field_deleted(ptr, key) } {
        return 1;
    }
    let attrs = unsafe { __torajs_obj_declared_field_attrs(ptr, key as *mut c_void) };
    if attrs >= 0 {
        if attrs as u64 & BUCKET_FLAG_CONFIGURABLE == 0 {
            return unsafe { refuse(throw_on_refusal) };
        }
        // The error `message` line swaps its own-absence sentinel in;
        // every other declared field records the deletion in its
        // sidecar (RFC 20260915) — the slot has nowhere to go, so the
        // own faces read the record before the slot.
        let swapped = unsafe { crate::struct_error_msg::error_message_delete(ptr, key) };
        if swapped != 0 {
            return swapped;
        }
        return unsafe { __torajs_obj_declared_field_delete(ptr, key as *mut c_void) };
    }
    let props = unsafe { crate::member_get_layout::struct_props(ptr) };
    if props.is_null() {
        // §13.5.1.2 — deleting a property that is not there succeeds.
        return 1;
    }
    if unsafe { refuse_non_configurable(props, key, throw_on_refusal) } {
        return 0;
    }
    // The property that replaced a deleted accessor (-2) goes back to
    // being a deletion record: removing the entry would bring the
    // layout's accessor back.
    if attrs == -2 {
        return unsafe { __torajs_obj_declared_field_delete(ptr, key as *mut c_void) };
    }
    unsafe { __torajs_dynobj_delete(props as *mut c_void, key) };
    1
}
