//! Bool-immediate receiver arm of the `any` method dispatcher —
//! split out of [`crate::method_call`] (blade 2a file-size account;
//! the arm itself stays a direct skeleton call, not a link seam).

use core::ffi::c_void;
use torajs_rc::{ANY_METHOD_TO_LOCALE_STRING, ANY_METHOD_TO_STRING, ANY_METHOD_VALUE_OF};

use crate::nanbox::{AnyValue, is_true};

unsafe extern "C" {
    /// torajs-str — fresh Str cell from raw bytes (rc=1).
    fn __torajs_str_alloc(data: *const u8, len: i64) -> *mut u8;
}

/// bool-immediate arm — `toString` / the inherited
/// `Object.prototype.toLocaleString` answer a fresh "true"/"false"
/// Str (ES §20.3.3.3), `valueOf` (§20.3.3.4) is the immediate
/// itself; 刀 2 (RFC 20260714-t262-top-clusters) — a generic
/// array-family mid reached through
/// `Array.prototype.every.call(true, …)` runs the empty-receiver
/// semantics (ToObject(bool) has no own `length` → every loop is
/// vacuous). Every other id is a TypeError.
pub(crate) unsafe fn bool_method(
    recv: AnyValue,
    mid: i64,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    if mid == ANY_METHOD_VALUE_OF {
        return recv;
    }
    if mid == ANY_METHOD_TO_STRING || mid == ANY_METHOD_TO_LOCALE_STRING {
        let bytes: &[u8] = if is_true(recv) { b"true" } else { b"false" };
        unsafe {
            let p = __torajs_str_alloc(bytes.as_ptr(), bytes.len() as i64);
            return crate::nanbox_encode::__torajs_anyv_box_pointer(p as *mut c_void);
        }
    }
    if crate::method_call_arraylike::arraylike_supported(mid)
        && !crate::method_call_arraylike_mut::arraylike_mut_supported(mid)
    {
        // §23.1.3 step 1 — ToObject(bool) mints the wrapper, whose
        // own-miss reads walk to the Boolean.prototype expando face
        // (`Boolean.prototype[1] = v; .length = 2`, RFC 20260721
        // G2d) — and the callback's O argument answers
        // `obj instanceof Boolean`. The wrapper is this frame's
        // temp, released after the scan.
        return unsafe {
            crate::method_call_arraylike::arraylike_on_minted_wrapper(recv, mid, argv, argc)
        };
    }
    if crate::method_call_arraylike_mut::arraylike_mut_supported(mid)
        && let Some(out) = unsafe {
            crate::method_call_arraylike_mut_prim::prim_mut_method(4, recv, mid, argv, argc)
        }
    {
        return out;
    }
    unsafe { crate::method_call::method_no_such() }
}
