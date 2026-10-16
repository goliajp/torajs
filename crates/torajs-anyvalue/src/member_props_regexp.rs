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
//! - any other object → an ordinary [[Get]] by the interned name (a
//!   user `{ source: "user" }` answers its value, a prototype
//!   accessor runs with the object as `this`).
//! - a primitive → `undefined`.

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
    /// torajs-regex — boxed-form lastIndex peek (BORROW; 0 = numeric
    /// form). The reader incs a heap cell per the boxed-value
    /// convention.
    fn __torajs_regex_last_index_raw(re: *const c_void) -> u64;
    fn __torajs_regex_has_flag(re: *const c_void, bit: i64) -> i64;
    /// torajs-rc — mint the reader's stake on a boxed-form heap cell.
    fn __torajs_rc_inc(p: *mut c_void);
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
        // An own `global` accessor (or one a program put on the
        // prototype) outranks the builtin the RegExp arm answers from.
        if tag == Tag::RegExp as u16
            && !(!key.is_null() && crate::regexp_protocol::shadowed(recv, key))
        {
            return match prop {
                p if p == ANY_RPROP_SOURCE => {
                    __torajs_anyv_box_from_pair(4, __torajs_regex_get_source(ptr) as i64)
                }
                p if p == ANY_RPROP_FLAGS => {
                    __torajs_anyv_box_from_pair(4, __torajs_regex_get_flags(ptr) as i64)
                }
                p if p == ANY_RPROP_LAST_INDEX => {
                    // §22.2.4.1 — a non-numeric any-lane store reads
                    // back VERBATIM (boxed overflow slot); numeric
                    // form boxes the f64 field as before.
                    let raw = __torajs_regex_last_index_raw(ptr);
                    if raw != 0 {
                        if is_cell(raw) {
                            __torajs_rc_inc(as_void_ptr(raw));
                        }
                        return raw;
                    }
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
        if !key.is_null() {
            // Every other object reads the name with an ordinary [[Get]]:
            // an own entry, or what it inherits — `RegExp.prototype.global`
            // runs the §22.2.6.5 getter on the prototype itself, and
            // `Object.create(RegExp.prototype).source` is its TypeError.
            return crate::member_get_receiver::__torajs_any_member_get_with_receiver(
                recv, key, recv,
            );
        }
    }
    VALUE_UNDEFINED
}

/// The §22.2.6.{4,5,6,7,8,9,10,11} flag accessors by NAME for the
/// runtime member face (`member_get::regexp_arm_tag` and its value
/// twin): a RegExp receiver read through a dynamic key (`re[k]`,
/// `Reflect.get(re, "global")`, the reified `get flags` body's own
/// eight Gets) answers the flag bit as a boolean — `Some(0 | 1)`, or
/// `None` for any other name. The static spelling keeps the interned
/// `ANY_RPROP_*` lane above.
///
/// # Safety
/// `ptr` is a live RegExp cell; `key` is NULL or a live Str cell.
pub(crate) unsafe fn regexp_flag_by_name(ptr: *mut c_void, key: *const c_void) -> Option<u64> {
    const FLAGS: [(&[u8], i64); 8] = [
        (b"hasIndices", 0x80),
        (b"global", 0x02),
        (b"ignoreCase", 0x01),
        (b"multiline", 0x04),
        (b"dotAll", 0x08),
        (b"unicode", 0x10),
        (b"unicodeSets", 0x40),
        (b"sticky", 0x20),
    ];
    if key.is_null() {
        return None;
    }
    for (name, bit) in FLAGS {
        if unsafe { crate::prop_has::key_is(key, name) } {
            return Some(unsafe { __torajs_regex_has_flag(ptr, bit) } as u64);
        }
    }
    None
}
