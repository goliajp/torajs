//! `__torajs_any_regexp_prop` — `r.source` / `r.lastIndex` /
//! `r.global`-family property reads where the receiver is an `any`
//! value (Any-method-call RFC 20260704 C4-3c-2).
//!
//! ssa-lower's member fallback interns the compile-time member name
//! into an `ANY_RPROP_*` id (torajs-rc) when it matches the RegExp
//! accessor surface, and routes here alongside the interned name
//! Str so non-RegExp receivers keep ordinary property semantics:
//!
//! - `null` / `undefined` → catchable TypeError.
//! - `Tag::RegExp` → the typed tier's accessor kernels
//!   (`ssa_lower_member_regexp_props.rs` parity): source / flags
//!   transfer a fresh +1 Str out, lastIndex boxes the f64 field,
//!   the six ES §22.2.6.5-10 booleans ride
//!   `__torajs_regex_has_flag` with the matching `RE_FLAG_*` bit.
//! - `Tag::DynObj` → own-property probe by the interned name (a
//!   user `{ source: "user" }` answers its value; absence answers
//!   `undefined`) — the `.length`/`.size` probe shape.
//! - everything else → `undefined` (no such property).

use core::ffi::c_void;

use torajs_rc::{
    ANY_RPROP_DOT_ALL, ANY_RPROP_FLAGS, ANY_RPROP_GLOBAL, ANY_RPROP_IGNORE_CASE,
    ANY_RPROP_LAST_INDEX, ANY_RPROP_MULTILINE, ANY_RPROP_SOURCE, ANY_RPROP_STICKY,
    ANY_RPROP_UNICODE, Tag,
};

use crate::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr, is_cell, is_null, is_undefined};
use crate::nanbox_encode::__torajs_anyv_box_from_pair;

unsafe extern "C" {
    /// torajs-regex — source / flags render fresh +1 Strs; the flag
    /// probe answers 0/1 for a `RE_FLAG_*` bit.
    fn __torajs_regex_get_source(re: *const c_void) -> *mut c_void;
    fn __torajs_regex_get_flags(re: *const c_void) -> *mut c_void;
    fn __torajs_regex_get_last_index(re: *const c_void) -> f64;
    fn __torajs_regex_has_flag(re: *const c_void, bit: i64) -> i64;
    /// torajs-dynobj — own-property probe pair ((5, 0) = absent).
    fn __torajs_dynobj_get_tag(obj: *const c_void, key: *const c_void) -> u64;
    fn __torajs_dynobj_get_value(obj: *const c_void, key: *const c_void) -> u64;
    /// torajs-throw — record a pending catchable TypeError.
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

// `RE_FLAG_*` bit mirrors (torajs-regex parser / the typed lowering
// in ssa_lower_member_regexp_props.rs).
const RE_FLAG_I: i64 = 0x01;
const RE_FLAG_G: i64 = 0x02;
const RE_FLAG_M: i64 = 0x04;
const RE_FLAG_S: i64 = 0x08;
const RE_FLAG_U: i64 = 0x10;
const RE_FLAG_Y: i64 = 0x20;

/// See module doc.
///
/// # Safety
/// Cell receivers are valid heap pointers; `key` is NULL or a live
/// Str cell for the DynObj probe.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_regexp_prop(
    recv: AnyValue,
    prop: i64,
    key: *const c_void,
) -> AnyValue {
    if is_null(recv) || is_undefined(recv) {
        unsafe {
            __torajs_throw_type_error(c"cannot read properties of null or undefined".as_ptr());
        }
        return VALUE_UNDEFINED;
    }
    if !is_cell(recv) {
        return VALUE_UNDEFINED;
    }
    let ptr = as_void_ptr(recv);
    unsafe {
        let tag = (ptr.cast::<u8>().add(4) as *const u16).read();
        if tag == Tag::RegExp as u16 {
            return match prop {
                p if p == ANY_RPROP_SOURCE => {
                    __torajs_anyv_box_from_pair(4, __torajs_regex_get_source(ptr) as i64)
                }
                p if p == ANY_RPROP_FLAGS => {
                    __torajs_anyv_box_from_pair(4, __torajs_regex_get_flags(ptr) as i64)
                }
                p if p == ANY_RPROP_LAST_INDEX => {
                    let li = __torajs_regex_get_last_index(ptr);
                    __torajs_anyv_box_from_pair(3, li.to_bits() as i64)
                }
                p => {
                    let bit = match p {
                        b if b == ANY_RPROP_GLOBAL => RE_FLAG_G,
                        b if b == ANY_RPROP_IGNORE_CASE => RE_FLAG_I,
                        b if b == ANY_RPROP_MULTILINE => RE_FLAG_M,
                        b if b == ANY_RPROP_DOT_ALL => RE_FLAG_S,
                        b if b == ANY_RPROP_UNICODE => RE_FLAG_U,
                        b if b == ANY_RPROP_STICKY => RE_FLAG_Y,
                        _ => return VALUE_UNDEFINED,
                    };
                    __torajs_anyv_box_from_pair(1, __torajs_regex_has_flag(ptr, bit))
                }
            };
        }
        if tag == Tag::DynObj as u16 && !key.is_null() {
            // The probe pair is a borrow — the returned box owns its
            // own reference (the `.length` / `.size` probe shape).
            let dtag = __torajs_dynobj_get_tag(ptr, key);
            let dval = __torajs_dynobj_get_value(ptr, key);
            crate::payload_rc_inc(dtag as i64, dval as i64);
            return __torajs_anyv_box_from_pair(dtag as i64, dval as i64);
        }
    }
    VALUE_UNDEFINED
}
