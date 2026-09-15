//! §25.1.6 / §25.3.4 — the accessor properties of
//! `ArrayBuffer.prototype` (`byteLength`, `maxByteLength`, `resizable`,
//! `detached`) and `DataView.prototype` (`buffer`, `byteLength`,
//! `byteOffset`). One table per prototype answers the key probe and
//! the own-name list; one dispatcher arm runs the getters.
//!
//! Each getter checks its own receiver (step 2-3 of every clause): a
//! value that is not an ArrayBuffer — a SharedArrayBuffer, a DataView,
//! a typed array, an ordinary object, the prototype itself — or not a
//! DataView respectively, is a TypeError. The detached / out-of-bounds
//! answers are the kernels' own.

use core::ffi::c_void;

use torajs_rc::Tag;
use torajs_rc::any_method_buffer as buf;

use crate::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr, box_bool, is_cell};

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    fn __torajs_arraybuffer_byte_length(av: AnyValue) -> i64;
    fn __torajs_arraybuffer_max_byte_length(av: AnyValue) -> i64;
    fn __torajs_arraybuffer_resizable(av: AnyValue) -> i64;
    fn __torajs_arraybuffer_detached(av: AnyValue) -> i64;
    fn __torajs_dataview_byte_length(av: AnyValue) -> i64;
    fn __torajs_dataview_byte_offset(av: AnyValue) -> i64;
    fn __torajs_dataview_buffer(av: AnyValue) -> AnyValue;
}

/// `ArrayBuffer.prototype`'s accessors as (getter id, name).
pub(crate) const ARRAYBUFFER_ACCESSORS: &[(i64, &str)] = &[
    (buf::ANY_METHOD_GET_AB_BYTE_LENGTH, "byteLength"),
    (buf::ANY_METHOD_GET_AB_MAX_BYTE_LENGTH, "maxByteLength"),
    (torajs_rc::ANY_METHOD_GET_RESIZABLE, "resizable"),
    (buf::ANY_METHOD_GET_AB_DETACHED, "detached"),
];

/// `DataView.prototype`'s accessors as (getter id, name).
pub(crate) const DATAVIEW_ACCESSORS: &[(i64, &str)] = &[
    (buf::ANY_METHOD_GET_DV_BUFFER, "buffer"),
    (buf::ANY_METHOD_GET_DV_BYTE_LENGTH, "byteLength"),
    (buf::ANY_METHOD_GET_DV_BYTE_OFFSET, "byteOffset"),
];

/// The getter id `key` names in `table`, if any.
///
/// # Safety
/// `key` is a live Str cell.
pub(crate) unsafe fn accessor_mid(table: &[(i64, &str)], key: *const c_void) -> Option<i64> {
    table
        .iter()
        .find(|(_, name)| unsafe { crate::prop_has::key_is(key, name.as_bytes()) })
        .map(|(mid, _)| *mid)
}

/// Is one of this module's getters able to run on any receiver (it
/// checks its own `this`)?
pub(crate) fn owns_getter(mid: i64) -> bool {
    mid == torajs_rc::ANY_METHOD_GET_RESIZABLE
        || (buf::ANY_METHOD_GET_AB_BYTE_LENGTH..=buf::ANY_METHOD_GET_DV_BYTE_OFFSET).contains(&mid)
}

fn has_tag(recv: AnyValue, tag: Tag) -> bool {
    // SAFETY: is_cell guarantees a live heap pointer with a header.
    is_cell(recv)
        && unsafe { as_void_ptr(recv).cast::<u8>().add(4).cast::<u16>().read() } == tag as u16
}

/// A reified ArrayBuffer / DataView prototype getter invoked on `recv`
/// — `None` when `mid` is not one of them. Owned answer.
///
/// # Safety
/// `recv` is a valid AnyValue.
pub(crate) unsafe fn buffer_accessor_call(recv: AnyValue, mid: i64) -> Option<AnyValue> {
    if !owns_getter(mid) {
        return None;
    }
    let is_dataview_getter = mid >= buf::ANY_METHOD_GET_DV_BUFFER;
    let brand = if is_dataview_getter {
        Tag::DataView
    } else {
        Tag::ArrayBuffer
    };
    if !has_tag(recv, brand) {
        let msg = if is_dataview_getter {
            c"this is not a DataView"
        } else {
            c"this is not an ArrayBuffer"
        };
        unsafe { __torajs_throw_type_error(msg.as_ptr()) };
        return Some(VALUE_UNDEFINED);
    }
    let n = |v: i64| crate::nanbox_encode::__torajs_anyv_box_i64(v);
    Some(unsafe {
        match mid {
            buf::ANY_METHOD_GET_AB_BYTE_LENGTH => n(__torajs_arraybuffer_byte_length(recv)),
            buf::ANY_METHOD_GET_AB_MAX_BYTE_LENGTH => n(__torajs_arraybuffer_max_byte_length(recv)),
            buf::ANY_METHOD_GET_AB_DETACHED => box_bool(__torajs_arraybuffer_detached(recv) != 0),
            buf::ANY_METHOD_GET_DV_BUFFER => __torajs_dataview_buffer(recv),
            buf::ANY_METHOD_GET_DV_BYTE_LENGTH => n(__torajs_dataview_byte_length(recv)),
            buf::ANY_METHOD_GET_DV_BYTE_OFFSET => n(__torajs_dataview_byte_offset(recv)),
            _ => box_bool(__torajs_arraybuffer_resizable(recv) != 0),
        }
    })
}
