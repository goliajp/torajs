//! A builtin prototype's virtual accessor read through a receiver its
//! family does not own — the prototype singleton itself
//! (`RegExp.prototype.global`) or an object that inherits from it
//! (`Object.create(Map.prototype).size`). §10.1.8.1 [[Get]] runs the
//! getter with the ORIGINAL receiver, and what that answers is the
//! getter's own business: the §22.2.6 RegExp getters answer undefined
//! on `RegExp.prototype` and throw on anything else, `get size` throws
//! on everything that is not a Map / Set.
//!
//! The receiver's own-family accessors never come here; they resolve
//! through `recv_family_accessor_mid`, whose brand is right by
//! construction.

use core::ffi::c_void;

use crate::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr, is_cell};

unsafe extern "C" {
    fn __torajs_anyv_get_proto_of_any(v: u64) -> u64;
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

/// The live (not delete-tombstoned) virtual accessor the builtin
/// prototype `tag` carries under `key`.
///
/// # Safety
/// `key` is NULL or a live Str cell.
pub(crate) unsafe fn live_accessor_mid(tag: i64, key: *const c_void) -> Option<i64> {
    let mid = unsafe { crate::method_support_proto::proto_tag_accessor_mid(tag, key) }?;
    (unsafe { torajs_rc::builtin_proto::__torajs_builtin_proto_is_deleted(tag, mid) } == 0)
        .then_some(mid)
}

/// Getters whose dispatcher arm performs the spec's own `this` checks
/// (and so may run on a receiver of another brand): the §22.2.6 RegExp
/// accessors, the four %TypedArray.prototype% views, and
/// `Symbol.prototype.description`'s thisSymbolValue, and the
/// ArrayBuffer / DataView prototype getters.
fn getter_checks_its_receiver(mid: i64) -> bool {
    use torajs_rc::any_method_buffer as buf;
    use torajs_rc::any_method_regexp as rx;
    mid == torajs_rc::ANY_METHOD_GET_FLAGS
        || mid == torajs_rc::ANY_METHOD_GET_DESCRIPTION
        || (rx::ANY_METHOD_GET_HAS_INDICES..=rx::ANY_METHOD_GET_SOURCE).contains(&mid)
        || (buf::ANY_METHOD_GET_TA_BUFFER..=buf::ANY_METHOD_GET_TA_LENGTH).contains(&mid)
        || crate::buffer_proto_accessors::owns_getter(mid)
}

/// Run getter `mid` on a receiver outside its family. Owned answer.
///
/// # Safety
/// `recv` is a live AnyValue.
pub(crate) unsafe fn run_foreign_accessor(recv: AnyValue, mid: i64) -> AnyValue {
    if !getter_checks_its_receiver(mid) {
        unsafe {
            __torajs_throw_type_error(
                c"builtin prototype accessor requires |this| to match its brand".as_ptr(),
            )
        };
        return VALUE_UNDEFINED;
    }
    unsafe {
        crate::method_call::__torajs_any_method_call(
            recv,
            mid,
            core::ptr::null(),
            0,
            core::ptr::null_mut(),
            core::ptr::null(),
            0,
        )
    }
}

/// The inherited leg: walk `recv` and its [[Prototype]] chain for the
/// builtin prototype singleton that carries `key` as a virtual
/// accessor, and run it on `recv`. `None` when no link carries one.
///
/// # Safety
/// `recv` is a live AnyValue; `key` is NULL or a live Str cell.
pub(crate) unsafe fn chain_virtual_accessor_get(
    recv: AnyValue,
    key: *const c_void,
) -> Option<AnyValue> {
    let mut cur = recv;
    let mut owned = false;
    let found = loop {
        if !is_cell(cur) {
            break None;
        }
        let tag =
            unsafe { torajs_rc::builtin_proto::__torajs_builtin_proto_tag_of(as_void_ptr(cur)) };
        if tag >= 0
            && let Some(mid) = unsafe { live_accessor_mid(tag, key) }
        {
            break Some(mid);
        }
        let next = unsafe { __torajs_anyv_get_proto_of_any(cur) };
        if owned {
            unsafe { crate::nanbox_ffi::__torajs_anyv_rc_dec(cur) };
        }
        if next == cur {
            // Both holds on the self-linked hop are released here; the
            // tail below must not release `cur` a third time.
            unsafe { crate::nanbox_ffi::__torajs_anyv_rc_dec(next) };
            owned = false;
            break None;
        }
        cur = next;
        owned = true;
    };
    if owned && is_cell(cur) {
        unsafe { crate::nanbox_ffi::__torajs_anyv_rc_dec(cur) };
    }
    Some(unsafe { run_foreign_accessor(recv, found?) })
}
