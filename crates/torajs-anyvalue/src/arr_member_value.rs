//! Typed `Array<T>` receiver named-member VALUE read (RFC
//! 20260721-array-proto-cluster 刀 3 / G5c) — `xs.toString` used to
//! answer undefined on a typed receiver because the T-29 props read
//! only consulted the per-array expando side table. §10.1.8.1
//! OrdinaryGet continues to the prototype after the own miss, so the
//! miss now falls through to the Array.prototype face
//! ([`crate::method_support_proto::__torajs_builtin_proto_method_value`]:
//! singleton own-entry / monkey-patch first, then the interned
//! reified method cell the any-lane dispatcher hands out — which is
//! what makes `xs.toString === Array.prototype.toString` hold).
//!
//! Answer is OWNED on every arm: an own-expando hit incs the payload
//! before boxing (the probe pair is a borrow), the proto face already
//! answers owned (interned cells are rc-immortal).
//!
//! The Array.prototype face is the whole prototype CHAIN as of the
//! knife that made `__torajs_builtin_proto_method_value` walk — it
//! stopped at the family before, so a typed array could not see
//! `Object.prototype.foo` while `const a: any = []` could.

use core::ffi::c_void;

use crate::AnyValue;
use crate::method_support_proto::__torajs_builtin_proto_method_value;

/// `torajs-rc/builtin_proto.rs` — `ARRAY_PROTO_TAG`.
const ARRAY_PROTO_TAG: i64 = 2;

unsafe extern "C" {
    /// torajs-arr props side table (link-time resolution, mirrors the
    /// wrapper-expando extern convention).
    fn __torajs_arrprops_has(arr: *mut c_void, key: *const c_void) -> i32;
    fn __torajs_arrprops_get_tag(arr: *mut c_void, key: *const c_void) -> u64;
    fn __torajs_arrprops_get_value(arr: *mut c_void, key: *const c_void) -> u64;
}

/// `xs.<name>` member VALUE read on a typed array receiver — own
/// expando entry first (an own undefined SHADOWS the proto face, RFC
/// 20260717 own-undefined-shadow family), then the Array.prototype
/// face. Owned result.
///
/// # Safety
/// `arr` is a live array heap block; `key` is NULL or a live Str cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_member_value(
    arr: *mut c_void,
    key: *const c_void,
) -> AnyValue {
    if !key.is_null() && unsafe { __torajs_arrprops_has(arr, key) } != 0 {
        let tag = unsafe { __torajs_arrprops_get_tag(arr, key) } as i64;
        let value = unsafe { __torajs_arrprops_get_value(arr, key) } as i64;
        crate::payload_rc_inc(tag, value);
        return unsafe { crate::nanbox_encode::__torajs_anyv_box_from_pair(tag, value) };
    }
    // §10.1.8.1 step 4 — the walk continues past `Array.prototype`:
    // the kernel below is the whole chain now, so a program's
    // `Object.prototype.foo = 5` reaches a TYPED array the same way
    // 517-07 made it reach an `any`-bound one.
    unsafe { __torajs_builtin_proto_method_value(ARRAY_PROTO_TAG, key) }
}
