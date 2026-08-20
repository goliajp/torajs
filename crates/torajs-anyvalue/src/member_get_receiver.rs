//! §10.1.8.1 OrdinaryGet with an explicit receiver — the read twin of
//! [`crate::member_set_receiver`].
//!
//! Every ordinary property read is `O.[[Get]](P, O)`: the object whose
//! chain is walked is also the `this` any getter runs against.
//! `Reflect.get(target, key, receiver)` (§28.1.6 step 4) is the only
//! spelling that pulls them apart, and the difference shows up in
//! exactly one place — an accessor answer. A data answer is whatever
//! the walk found on the target, receiver or no receiver.

use core::ffi::c_void;

use crate::nanbox::AnyValue;

unsafe extern "C" {
    /// torajs-dynobj — getter dispatch against an AccessorPair cell.
    fn __torajs_accessor_invoke_getter(pair: *const c_void, recv_anyv: u64) -> u64;
}

/// `target.[[Get]](key, receiver)`. The result is OWNED — a data
/// answer is converted from the walk's borrow before boxing, and a
/// getter's return carries its own reference.
///
/// Boundary (recorded, not silently wrong): a STRUCT-lane accessor —
/// one declared on a class, resolved through the struct layout rather
/// than through an AccessorPair cell — is invoked against the object
/// that owns the layout, so a differing receiver does not reach it.
/// That lane reads the getter out of the receiver's own class
/// metadata; handing it a receiver of another shape would not be a
/// different `this`, it would be a different getter. The dynobj lane,
/// which is every object literal and everything `Object.defineProperty`
/// builds, takes the receiver.
///
/// # Safety
/// `target` and `receiver` are AnyValues; `key` is a live Str or
/// Symbol key cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_member_get_with_receiver(
    target: u64,
    key: *const c_void,
    receiver: u64,
) -> AnyValue {
    unsafe {
        let tag = crate::member_get::__torajs_any_member_get_tag(target, key);
        if tag == crate::struct_probe::ANY_ACCESSOR_TAG {
            let pair_bits = crate::member_get_value::__torajs_any_member_get_value(target, key);
            if pair_bits != 0 && receiver != target {
                return __torajs_accessor_invoke_getter(pair_bits as *const c_void, receiver);
            }
            return crate::struct_probe::__torajs_any_accessor_get(target, key, pair_bits);
        }
        let payload = crate::member_get_value::__torajs_any_member_get_value(target, key);
        crate::payload_rc_inc(tag as i64, payload as i64);
        crate::nanbox_encode::__torajs_anyv_box_from_pair(tag as i64, payload as i64)
    }
}
