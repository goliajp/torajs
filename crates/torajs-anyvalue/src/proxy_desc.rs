//! §6.2.6.6 CompletePropertyDescriptor for a trap's answer
//! (RFC 20260823-proxy-substrate 刀 4).
//!
//! §10.5.5 step 12 runs the `getOwnPropertyDescriptor` trap's result
//! through CompletePropertyDescriptor before answering, so a handler
//! that returns `{ value: 1 }` is observed as
//! `{ value: 1, writable: false, enumerable: false, configurable:
//! false }`. Without this a caller reading `.enumerable` gets
//! `undefined`, which is falsish and therefore *usually* right —
//! which is exactly why it would have gone unnoticed.

use crate::nanbox::{AnyValue, VALUE_UNDEFINED};

/// Fill the absent attributes of a descriptor object in place and
/// answer it (ownership passes through unchanged).
///
/// A descriptor naming `get` or `set` is an ACCESSOR descriptor and
/// completes with the other accessor half undefined; every other one
/// is a data descriptor and completes with `value: undefined` and
/// `writable: false`. Both complete `enumerable` / `configurable`
/// to false.
///
/// # Safety
/// `desc` is a live object AnyValue owned by the caller.
pub(crate) unsafe fn complete_descriptor(desc: AnyValue) -> AnyValue {
    unsafe {
        let mut slot = desc;
        let is_accessor = has_field(slot, b"get") || has_field(slot, b"set");
        if is_accessor {
            fill(&mut slot, b"get", VALUE_UNDEFINED);
            fill(&mut slot, b"set", VALUE_UNDEFINED);
        } else {
            fill(&mut slot, b"value", VALUE_UNDEFINED);
            fill(&mut slot, b"writable", crate::nanbox::box_bool(false));
        }
        fill(&mut slot, b"enumerable", crate::nanbox::box_bool(false));
        fill(&mut slot, b"configurable", crate::nanbox::box_bool(false));
        slot
    }
}

/// Does `desc` carry this attribute at all? §6.2.6.5 asks
/// HasProperty, not "is it truthy" — `{ get: undefined }` IS an
/// accessor descriptor.
unsafe fn has_field(desc: AnyValue, name: &[u8]) -> bool {
    unsafe {
        let key = __torajs_str_alloc(name.as_ptr(), name.len() as i64);
        let r = crate::prop_has::__torajs_any_has_property(desc, key as *const core::ffi::c_void);
        __torajs_str_drop(key as *mut core::ffi::c_void);
        r != 0
    }
}

/// Write `value` under `name` when the field is absent. `slot` is
/// the caller's live handle on the descriptor — a dynobj resize
/// relocates the block and the set core writes the new address back
/// through it, so every later read has to go through the same slot.
unsafe fn fill(slot: &mut AnyValue, name: &[u8], value: AnyValue) {
    unsafe {
        if has_field(*slot, name) {
            return;
        }
        let key = __torajs_str_alloc(name.as_ptr(), name.len() as i64);
        let (tag, payload) = (
            crate::__torajs_anyv_unbox_tag(value),
            crate::__torajs_anyv_unbox_value(value),
        );
        crate::payload_rc_inc(tag, payload);
        crate::member_set::__torajs_any_member_set(
            slot,
            key as *mut core::ffi::c_void,
            tag as u64,
            payload as u64,
            -1,
        );
        __torajs_str_drop(key as *mut core::ffi::c_void);
    }
}

unsafe extern "C" {
    fn __torajs_str_alloc(bytes: *const u8, len: i64) -> *mut u8;
    fn __torajs_str_drop(s: *mut core::ffi::c_void);
}
