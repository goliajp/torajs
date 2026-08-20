//! `__torajs_any_member_set_priv` — the brand-gated write channel a
//! `#x` member WRITE lowers to (spec §7.3.32 PrivateSet: writing a
//! private element to an object whose class did not declare it
//! throws TypeError, never installs an expando).
//!
//! The lowering picks this channel STATICALLY (`__priv_` prefix on
//! the member name in `emit_any_member_set`), so the ordinary member
//! write hot path pays nothing. Unlike the read twin
//! (`member_get_private`) this shell must gate BEFORE deferring —
//! the base kernel's miss tail happily installs an expando entry,
//! which is exactly the silent-wrong PrivateSet forbids. A declared
//! brand (layout field / expando on a degraded instance / dynobj
//! entry / prototype-face accessor) defers to the base kernel, whose
//! accessor dispatch runs a private setter and raises the readonly
//! TypeError for a getter-only pair. Primitives and nullish
//! receivers carry no brand and throw here (PrivateSet's ToObject-
//! free receiver check).

use crate::member_get_private::priv_brand_declared;
use crate::nanbox::AnyValue;
use core::ffi::c_void;

unsafe extern "C" {
    fn __torajs_any_member_set(
        recv_slot: *mut AnyValue,
        key: *mut c_void,
        tag: u64,
        value: u64,
        hint: i64,
    );
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

/// See module doc.
///
/// # Safety
/// `recv_slot` points at a live AnyValue slot; `key` is a live Str
/// cell (private names are never symbols).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_member_set_priv(
    recv_slot: *mut AnyValue,
    key: *mut c_void,
    tag: u64,
    value: u64,
    hint: i64,
) {
    let recv = unsafe { *recv_slot };
    if !unsafe { priv_brand_declared(recv, key) } {
        unsafe {
            __torajs_throw_type_error(
                c"cannot write private member to an object whose class did not declare it".as_ptr(),
            );
        }
        return;
    }
    unsafe { __torajs_any_member_set(recv_slot, key, tag, value, hint) }
}
