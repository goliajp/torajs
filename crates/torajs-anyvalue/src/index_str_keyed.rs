//! A typed string's read at a key its index view does not answer.
//!
//! `s[1.5]` names the property "1.5" and `s[7]` past the end names
//! "7": neither is an own property of the String exotic object
//! (§10.4.3.5 StringGetOwnProperty answers only the CanonicalNumeric
//! integer keys below the length), so §10.1.8.1 OrdinaryGet goes on
//! to %String.prototype% and %Object.prototype%. The typed lane asks
//! here only when the program can put a property on those chains;
//! otherwise such a read is `undefined` without a call.

use core::ffi::c_void;

use crate::index_any_keyed::__torajs_any_index_get_keyed;
use crate::nanbox::{AnyValue, as_void_ptr, box_double, box_void_ptr, is_cell, is_undefined};

unsafe extern "C" {
    fn __torajs_substr_undef() -> *mut u8;
    fn __torajs_substr_create(parent: *mut c_void, offset: u64, len: u64) -> *mut c_void;
    fn __torajs_str_drop(s: *mut c_void);
    /// torajs-str — the code unit at `idx` as a fresh view, NULL past
    /// either end.
    fn __torajs_str_index_get(s: *mut u8, idx: i64) -> *mut u8;
}

/// `FLAG_SUBSTR_VIEW` (torajs-str `substr.rs`).
const FLAG_SUBSTR_VIEW: u16 = 1 << 10;

/// `s[key]` for a `string` receiver (a Str cell or a Substr view),
/// answered in the lane's Substr shape: an in-range integer key reads
/// the code unit, any other key reads the property it names through
/// the keyed kernel, which runs a getter and records its throw.
///
/// # Safety
/// `s` is a live Str or Substr cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_index_keyed(s: *mut c_void, key: f64) -> *mut c_void {
    unsafe {
        if key >= 0.0 && key.trunc() == key && key < 4_294_967_295.0 {
            let own = __torajs_str_index_get(s as *mut u8, key as i64);
            if !own.is_null() {
                return own as *mut c_void;
            }
        }
        substr_answer(__torajs_any_index_get_keyed(
            box_void_ptr(s),
            box_double(key),
        ))
    }
}

/// The owned answer `v` as a Substr: `undefined` is the lane's
/// sentinel view, a view passes through, and a string or another
/// primitive is its ToString viewed whole. An object answer has no
/// spelling in a string slot; it is released and reads `undefined`
/// (the lane's recorded approximation, like the `number[]` lane's
/// ToNumber).
unsafe fn substr_answer(v: AnyValue) -> *mut c_void {
    unsafe {
        if is_undefined(v) {
            return __torajs_substr_undef() as *mut c_void;
        }
        if is_cell(v) {
            let p = as_void_ptr(v);
            let tag = (p as *const u8).add(4).cast::<u16>().read();
            let flags = (p as *const u8).add(6).cast::<u16>().read();
            if tag == torajs_rc::Tag::Str as u16 && flags & FLAG_SUBSTR_VIEW != 0 {
                return p;
            }
            if crate::to_primitive::is_object_value(v) {
                crate::nanbox_ffi::__torajs_anyv_rc_dec(v);
                return __torajs_substr_undef() as *mut c_void;
            }
        }
        let st = crate::nanbox_ffi::__torajs_anyv_to_str(v);
        crate::nanbox_ffi::__torajs_anyv_rc_dec(v);
        let len = (st as *const u8).add(8).cast::<u32>().read() as u64;
        let view = __torajs_substr_create(st, 0, len);
        __torajs_str_drop(st);
        view
    }
}
