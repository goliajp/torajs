//! ToString / ToNumber-coercing global arms of the ns-static
//! dispatch family ([`super::ns_static`]) — the §19.2 function
//! properties of the global object whose step 1 coerces the
//! argument: `parseInt` / `parseFloat` (§19.2.5/.4), the global
//! `isFinite` / `isNaN` (§19.2.2/.3), and the four §19.2.6 URI
//! kernels. Split out of `ns_static.rs` when the URI arms would
//! have pushed it over the 500-line cap (same mechanical-move
//! story as `ns_static_util`).

use core::ffi::c_void;

use crate::nanbox::{VALUE_UNDEFINED, box_double};
use crate::nanbox_encode::__torajs_anyv_box_str_slot;

use super::ns_static::{arg_at, arg_num, to_i64_mod32};
use super::ns_static_table::{
    __torajs_num_parse_float, __torajs_num_parse_int, __torajs_str_drop, __torajs_throw_check,
};
use super::ns_static_util::box_bool;

unsafe extern "C" {
    /// torajs-str `uri.rs` — the §19.2.6 Encode / Decode kernels
    /// ((Str, component flag) → fresh Str; malformed input records
    /// a pending URIError and answers the empty placeholder).
    fn __torajs_str_uri_encode(s: *const u8, component: i64) -> *mut u8;
    fn __torajs_str_uri_decode(s: *const u8, component: i64) -> *mut u8;
}

/// §19.2.5 parseInt arm — ToString the input (may throw), ToInt32
/// the radix, then the typed-tier parse kernel.
pub(super) unsafe fn parse_int_value(argv: *const u64, argc: i64) -> u64 {
    unsafe {
        let s = crate::nanbox_ffi::__torajs_anyv_to_str(arg_at(argv, argc, 0));
        if __torajs_throw_check() != 0 {
            return VALUE_UNDEFINED;
        }
        let Ok(radix) = arg_num(argv, argc, 1) else {
            __torajs_str_drop(s);
            return VALUE_UNDEFINED;
        };
        let n = __torajs_num_parse_int(s as *const u8, to_i64_mod32(radix));
        __torajs_str_drop(s);
        box_double(n)
    }
}

/// §19.2.4 parseFloat arm — ToString the input (may throw), then
/// the typed-tier parse kernel.
pub(super) unsafe fn parse_float_value(argv: *const u64, argc: i64) -> u64 {
    unsafe {
        let s = crate::nanbox_ffi::__torajs_anyv_to_str(arg_at(argv, argc, 0));
        if __torajs_throw_check() != 0 {
            return VALUE_UNDEFINED;
        }
        let n = __torajs_num_parse_float(s as *const u8);
        __torajs_str_drop(s);
        box_double(n)
    }
}

/// §19.2.2/.3 global isFinite / isNaN arm — ToNumber first (unlike
/// the Number.* predicates), so a Symbol / BigInt argument throws
/// and the 0-arg call tests ToNumber(undefined) = NaN.
pub(super) unsafe fn global_num_test(finite: bool, argv: *const u64, argc: i64) -> u64 {
    unsafe {
        let n = crate::nanbox_ffi::__torajs_anyv_to_number(arg_at(argv, argc, 0));
        if __torajs_throw_check() != 0 {
            return VALUE_UNDEFINED;
        }
        box_bool(if finite { n.is_finite() } else { n.is_nan() })
    }
}

/// §19.2.6 URI arm — ToString the argument (may throw), the
/// torajs-str kernel (URIError on a malformed input), then the
/// owned-Str slot box.
pub(super) unsafe fn uri_kernel_value(
    encode: bool,
    component: bool,
    argv: *const u64,
    argc: i64,
) -> u64 {
    unsafe {
        let s = crate::nanbox_ffi::__torajs_anyv_to_str(arg_at(argv, argc, 0));
        if __torajs_throw_check() != 0 {
            return VALUE_UNDEFINED;
        }
        let out = if encode {
            __torajs_str_uri_encode(s as *const u8, i64::from(component))
        } else {
            __torajs_str_uri_decode(s as *const u8, i64::from(component))
        };
        __torajs_str_drop(s);
        if __torajs_throw_check() != 0 {
            // the kernel's answer is the empty-Str placeholder —
            // reclaim it and surface the pending URIError.
            __torajs_str_drop(out as *mut c_void);
            return VALUE_UNDEFINED;
        }
        __torajs_anyv_box_str_slot(out.cast::<c_void>())
    }
}
