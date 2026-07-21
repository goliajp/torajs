//! `Array.prototype.concat` generic arm for NON-array OBJECT
//! receivers (RFC 20260721-array-proto-cluster 刀 8-B) — the
//! reified-cell `.call` / `.apply` re-dispatch gate entry for cell
//! receivers (dynobj / struct / chain hosts).
//!
//! §23.1.3.1 for a non-array `this`: `ArraySpeciesCreate(O, 0)`
//! answers a plain `ArrayCreate` (`IsArray(O)` is false — the
//! receiver's `constructor` is never consulted), then O itself
//! appends as the single seed element (no `@@isConcatSpreadable`
//! surface, so only real Arrays spread) and each argument spreads
//! or appends. `Get(O, "length")` never runs — concat has no
//! length read, so this entry dispatches BEFORE
//! [`crate::method_call_arraylike::arraylike_method`]'s observable
//! length-getter step.
//!
//! Cell receivers only: the primitive wrapper-proto route
//! ([`crate::method_call_arraylike::arraylike_on_wrapper_proto`])
//! keeps concat excluded — tr has no Boolean/Number wrapper object
//! to seed, and answering the proto singleton would be
//! silent-wrong; the not-callable TypeError stays loud there.

use core::ffi::c_void;

use crate::nanbox::AnyValue;
use crate::nanbox_encode::__torajs_anyv_box_pointer;

unsafe extern "C" {
    /// Cross-tier — torajs-arr seeded concat kernel (receiver as
    /// the single non-spreadable seed element; fresh +1 return).
    fn __torajs_arr_any_concat_generic(recv: u64, argv: *const u64, argc: i64) -> *mut u8;
}

/// The array-family mids a CELL receiver's re-dispatch gate admits:
/// everything [`crate::method_call_arraylike::arraylike_supported`]
/// covers plus concat (which needs a receiver identity to seed).
pub(crate) fn obj_supported(mid: i64) -> bool {
    crate::method_call_arraylike::arraylike_supported(mid) || mid == torajs_rc::ANY_METHOD_CONCAT
}

/// Gate entry for the cell-receiver re-dispatch sites: concat runs
/// its own §23.1.3.1 shape here; every other mid delegates to the
/// generic array-like arm (length-first semantics unchanged).
///
/// # Safety
/// `obj` is a valid live heap cell; `argv` holds `argc` BORROWED
/// NaN-box AnyValues; `recv_slot` is NULL or a valid writeback slot.
pub(crate) unsafe fn obj_method(
    obj: *mut c_void,
    mid: i64,
    recv_slot: *mut u64,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    unsafe {
        if mid == torajs_rc::ANY_METHOD_CONCAT {
            let p = __torajs_arr_any_concat_generic(__torajs_anyv_box_pointer(obj), argv, argc);
            return __torajs_anyv_box_pointer(p as *mut c_void);
        }
        crate::method_call_arraylike::arraylike_method(obj, mid, recv_slot, argv, argc)
    }
}
