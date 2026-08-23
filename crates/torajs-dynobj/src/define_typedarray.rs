//! §10.4.5.3 / bag dispatch for a buffer-family [[DefineOwnProperty]]
//! receiver, split from `define.rs` (file-size cap; RFC
//! 20260823-typedarray-substrate numeric-element-face knife). A
//! canonical numeric spelling on a typed array is the ELEMENT face;
//! every other key recurses into the lazy expando bag like the
//! Promise arm.

use core::ffi::c_void;

use crate::define::{bag_receiver_define, drop_rejected_value};
use crate::layout::DEFINE_PRESENT_VALUE;

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const u8);
    fn __torajs_throw_check() -> i64;
    /// torajs-anyvalue — borrow-shaped cell box (no rc traffic).
    fn __torajs_anyv_box_pointer(p: *mut c_void) -> u64;
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
    fn __torajs_anyv_rc_dec(v: u64);
    /// torajs-anyvalue — §7.1.21 CanonicalNumericIndexString probe
    /// (1 = numeric, `out` written).
    fn __torajs_buffer_canonical_numeric_index(key: *const c_void, out: *mut f64) -> i64;
    /// torajs-buffer — the view's CURRENT element count and the
    /// §10.4.5.5 element store (coerce first).
    fn __torajs_typedarray_length(v: u64) -> i64;
    fn __torajs_typedarray_index_set(recv: u64, idx: f64, v: u64);
}

/// The buffer family's expando slot for its header tag
/// (torajs-buffer `PROPS_OFF` mirrors).
fn buffer_props_off(htag: u16) -> usize {
    if htag == crate::layout::TAG_TYPEDARRAY_HDR {
        crate::layout::TYPEDARRAY_PROPS_OFF
    } else {
        crate::layout::ARRAYBUFFER_PROPS_OFF
    }
}

/// `define_apply`'s buffer-family arm.
///
/// # Safety
/// `obj` is a live buffer-family cell of `htag`; the rest per
/// `define_apply`'s contract.
#[allow(clippy::too_many_arguments)]
pub(crate) unsafe fn buffer_receiver_define(
    obj: *mut c_void,
    htag: u16,
    key: *mut c_void,
    tag: u64,
    value: u64,
    flags_byte: u64,
    throw_on_refusal: bool,
) -> i64 {
    unsafe {
        if htag == crate::layout::TAG_TYPEDARRAY_HDR {
            let mut n: f64 = 0.0;
            if __torajs_buffer_canonical_numeric_index(key, &mut n) != 0 {
                return typedarray_numeric_define(obj, n, tag, value, flags_byte, throw_on_refusal);
            }
        }
        let off = buffer_props_off(htag);
        bag_receiver_define(obj, off, key, tag, value, flags_byte, throw_on_refusal)
    }
}

/// §10.4.5.3 [[DefineOwnProperty]] over a canonical numeric key of
/// a typed array (steps 1.b.i-vii): an invalid integer index — out
/// of bounds against the CURRENT length, `-0`, a non-integer — is a
/// refusal; so are an accessor descriptor and an explicit
/// `configurable` / `enumerable` / `writable` of false (the element
/// face is {writable, enumerable, configurable: true}); a present
/// [[Value]] runs IntegerIndexedElementSet (coerce, then store).
unsafe fn typedarray_numeric_define(
    obj: *mut c_void,
    n: f64,
    tag: u64,
    value: u64,
    flags_byte: u64,
    throw_on_refusal: bool,
) -> i64 {
    use crate::layout::{
        DEFINE_FLAG_CONFIGURABLE, DEFINE_FLAG_ENUMERABLE, DEFINE_FLAG_WRITABLE,
        DEFINE_PRESENT_CONFIGURABLE, DEFINE_PRESENT_ENUMERABLE, DEFINE_PRESENT_GET,
        DEFINE_PRESENT_SET, DEFINE_PRESENT_WRITABLE,
    };
    unsafe {
        let recv = __torajs_anyv_box_pointer(obj);
        let len = __torajs_typedarray_length(recv);
        let valid =
            n.fract() == 0.0 && !(n == 0.0 && n.is_sign_negative()) && n >= 0.0 && (n as i64) < len;
        let refuse = !valid
            || flags_byte & (DEFINE_PRESENT_GET | DEFINE_PRESENT_SET) != 0
            || (flags_byte & DEFINE_PRESENT_CONFIGURABLE != 0
                && flags_byte & DEFINE_FLAG_CONFIGURABLE == 0)
            || (flags_byte & DEFINE_PRESENT_ENUMERABLE != 0
                && flags_byte & DEFINE_FLAG_ENUMERABLE == 0)
            || (flags_byte & DEFINE_PRESENT_WRITABLE != 0
                && flags_byte & DEFINE_FLAG_WRITABLE == 0);
        if refuse {
            if flags_byte & DEFINE_PRESENT_VALUE != 0 {
                drop_rejected_value(tag, value);
            }
            if throw_on_refusal && __torajs_throw_check() == 0 {
                __torajs_throw_type_error(
                    c"Invalid typed array index descriptor.".as_ptr() as *const u8
                );
            }
            return 0;
        }
        if flags_byte & DEFINE_PRESENT_VALUE != 0 {
            let boxed = __torajs_anyv_box_from_pair(tag as i64, value as i64);
            __torajs_typedarray_index_set(recv, n, boxed);
            __torajs_anyv_rc_dec(boxed);
            if __torajs_throw_check() != 0 {
                return 0;
            }
        }
        1
    }
}
