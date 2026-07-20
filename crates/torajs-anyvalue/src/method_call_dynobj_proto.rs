//! Inherited `Object.prototype` surface for plain-object receivers —
//! shared tail split from `method_call_dynobj.rs` (rotation 119
//! chunk 10, file-size decomp: parent 500 → ~410, back to soft-warn
//! safe zone).
//!
//! Both `dynobj_method` (Tag::DynObj probe) and `struct_method`
//! (Tag::Obj static-layout probe) fall through here after the
//! own-property probe misses, so a user monkey-patch always wins.
//!
//! - `object_proto_fallback` — the shared prototype dispatch:
//!   builtin-proto primitive fast path, `valueOf` (§20.1.4.7),
//!   `toString` (§20.1.3.6 / Error §20.5.3.4 / badge cell),
//!   `toLocaleString` (§20.1.4.6 re-dispatch as toString).
//! - `builtin_proto_primitive` — Number.prototype / String.prototype
//!   / Boolean.prototype answers per §21.1.3 / §22.1.3 / §20.3.3.
//! - `str_any` — allocate a fresh Str cell and box it as AnyValue.

use core::ffi::c_void;

use torajs_rc::{
    __torajs_rc_inc, ANY_METHOD_TO_LOCALE_STRING, ANY_METHOD_TO_STRING, ANY_METHOD_VALUE_OF,
};

use crate::method_call::not_callable;
use crate::method_call_dynobj::{dynobj_method, struct_method};
use crate::nanbox::AnyValue;
use crate::nanbox_encode::__torajs_anyv_box_pointer;

unsafe extern "C" {
    fn __torajs_str_alloc(src: *const u8, len: i64) -> *mut u8;
    fn __torajs_str_drop(s: *mut c_void);
}

/// Inherited `Object.prototype` surface for plain-object receivers —
/// runs only AFTER the own-property probe missed, so user
/// monkey-patches always win. `valueOf` (§20.1.4.7) answers the
/// receiver itself (fresh +1 per the boxed-value convention);
/// `toString` (§20.1.3.6) answers the "[object Object]" text;
/// `toLocaleString` (§20.1.4.6 "invoke this.toString") re-dispatches
/// as a toString call so a user own `toString` wins there too.
///
/// # Safety
/// Callers hold a live receiver ptr matching the `is_struct` flag
/// (Tag::Obj vs Tag::DynObj) and a valid `argv`.
pub(crate) unsafe fn object_proto_fallback(
    obj: *mut c_void,
    mid: i64,
    is_struct: bool,
    argv: *const u64,
) -> AnyValue {
    unsafe {
        // ES §21.1.3 / §20.3.3 / §22.1.3 — `Number.prototype` IS a Number
        // object ([[NumberData]] = +0), `Boolean.prototype` a Boolean one
        // (false), `String.prototype` a String one (""). tr builds every
        // builtin prototype as an empty dynobj, so `Number.prototype
        // .toString()` fell through to here and answered "[object Object]"
        // where the spec says "0" — which is the FIRST assertion of most
        // Number/prototype/toString cases, so the whole family died on it.
        // Ordered after the own probe like the rest of this fallback: a
        // monkey-patched `Number.prototype.toString` still wins. A
        // `delete String.prototype.toString` tombstone (RFC 20260721
        // G9) retires the primitive-identity face too — the call then
        // inherits the §20.1.3.6 badge below ("[object String]").
        if !is_struct
            && !proto_family_mid_deleted(obj, mid)
            && let Some(v) = builtin_proto_primitive(obj, mid)
        {
            return v;
        }
        if mid == ANY_METHOD_VALUE_OF {
            __torajs_rc_inc(obj);
            return __torajs_anyv_box_pointer(obj);
        }
        if mid == ANY_METHOD_TO_LOCALE_STRING {
            let key = __torajs_str_alloc(b"toString".as_ptr(), 8);
            let out = if is_struct {
                struct_method(obj, ANY_METHOD_TO_STRING, key as *const u8, argv, 0)
            } else {
                dynobj_method(
                    obj,
                    ANY_METHOD_TO_STRING,
                    key as *const u8,
                    core::ptr::null_mut(),
                    argv,
                    0,
                )
            };
            __torajs_str_drop(key as *mut c_void);
            return out;
        }
        if mid == ANY_METHOD_TO_STRING {
            // Error.prototype.toString (§20.5.3.4) — a FLAG_ERROR
            // struct answers `name: message` (matching the SSA
            // typed-tier lowering), not the "[object Object]" badge.
            // After the own-property probe, so a monkey-patched own
            // `toString` still wins.
            if is_struct
                && let Some(v) = crate::method_call_object_proto::error_struct_tostring(obj)
            {
                return v;
            }
            // §20.1.3.6 through the badge classifier rather than a
            // hardcoded "[object Object]": the five container
            // prototypes carry a well-known `Symbol.toStringTag`, so
            // `Map.prototype.toString()` is "[object Map]".
            return crate::method_call_object_proto::cell_badge_string(obj, is_struct);
        }
        not_callable()
    }
}

/// True iff the receiver is a builtin prototype singleton whose
/// family method under `mid` has been `delete`d (the torajs-rc
/// deleted-mid tombstone) — the caller then skips the
/// primitive-identity face and inherits the ordinary
/// `Object.prototype` surface.
unsafe fn proto_family_mid_deleted(obj: *mut c_void, mid: i64) -> bool {
    let ptag = unsafe { torajs_rc::builtin_proto::__torajs_builtin_proto_tag_of(obj) };
    ptag >= 0
        && unsafe { torajs_rc::builtin_proto::__torajs_builtin_proto_is_deleted(ptag, mid) } != 0
}

/// The primitive a builtin prototype carries as its internal slot, for
/// the two methods that read it. `None` for every other receiver (and
/// for the prototypes with no primitive data — `Object.prototype`,
/// `Map.prototype`, ... — which keep the ordinary Object.prototype
/// surface), so the caller falls through unchanged.
///
/// `Array.prototype` is deliberately absent: the spec makes it an ARRAY
/// (an empty one), not a primitive wrapper, so `Array.prototype
/// .toString()` answering "" has to come from the array lane, not here.
unsafe fn builtin_proto_primitive(obj: *mut c_void, mid: i64) -> Option<AnyValue> {
    // Tags are ssa_lower's, fixed by `torajs_rc::builtin_proto`:
    // Number=0, String=3, Boolean=4.
    let tag = unsafe { torajs_rc::builtin_proto::__torajs_builtin_proto_tag_of(obj) };
    let is_to_string = mid == ANY_METHOD_TO_STRING;
    if !is_to_string && mid != ANY_METHOD_VALUE_OF {
        return None;
    }
    match tag {
        // §21.1.3.6 — `Number.prototype.toString(radix)` on +0 is "0" in
        // every radix, so the arg needs no inspection.
        0 if is_to_string => Some(unsafe { str_any(b"0") }),
        0 => Some(crate::nanbox_encode::__torajs_anyv_box_f64(0.0)),
        3 if is_to_string => Some(unsafe { str_any(b"") }),
        3 => Some(unsafe { str_any(b"") }),
        4 if is_to_string => Some(unsafe { str_any(b"false") }),
        4 => Some(crate::nanbox_encode::__torajs_anyv_box_bool(0)),
        _ => None,
    }
}

/// A fresh owned Str cell boxed as an AnyValue.
unsafe fn str_any(bytes: &[u8]) -> AnyValue {
    unsafe {
        let p = __torajs_str_alloc(bytes.as_ptr(), bytes.len() as i64);
        __torajs_anyv_box_pointer(p as *mut c_void)
    }
}
