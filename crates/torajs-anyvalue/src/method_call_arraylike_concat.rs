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
//! Primitive receivers ride [`prim_method`]: §23.1.3.1 step 1
//! `ToObject(this)` mints the Number/Boolean/String wrapper object
//! (RFC 20260716 刀 4) as the non-spreadable seed —
//! `Array.prototype.concat.call(true)[0] instanceof Boolean`. The
//! wrapper-proto route
//! ([`crate::method_call_arraylike::arraylike_on_wrapper_proto`])
//! still excludes concat: its host is the proto singleton, never a
//! receiver identity to seed.

use core::ffi::c_void;

use crate::nanbox::{AnyValue, as_void_ptr};
use crate::nanbox_encode::__torajs_anyv_box_pointer;

unsafe extern "C" {
    /// Cross-tier — torajs-arr seeded concat kernel (receiver as
    /// the single non-spreadable seed element; fresh +1 return).
    fn __torajs_arr_any_concat_generic(recv: u64, argv: *const u64, argc: i64) -> *mut u8;
    /// Cross-tier — universal NaN-box-safe heap-value release.
    fn __torajs_value_drop_heap(p: *mut c_void);
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

/// `Array.prototype.concat.call(prim, ...items)` — §23.1.3.1 step 1
/// ToObject(this) mints the wrapper object, then the seeded kernel
/// runs the same shape as the cell arm. The wrapper is this frame's
/// temp: the seed slot takes its own stake, ours releases after.
///
/// # Safety
/// `recv` is a BORROWED primitive-shaped AnyValue (never nullish —
/// callers gate); `argv` holds `argc` BORROWED NaN-box AnyValues.
pub(crate) unsafe fn prim_method(recv: AnyValue, argv: *const u64, argc: i64) -> AnyValue {
    unsafe {
        let w = crate::to_object::__torajs_any_to_object(recv);
        let p = __torajs_arr_any_concat_generic(w, argv, argc);
        __torajs_value_drop_heap(as_void_ptr(w));
        __torajs_anyv_box_pointer(p as *mut c_void)
    }
}
