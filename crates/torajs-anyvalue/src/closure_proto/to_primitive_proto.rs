//! The two builtin `[Symbol.toPrimitive]` methods — §20.4.3.5
//! `Symbol.prototype[@@toPrimitive]` and §21.4.4.45
//! `Date.prototype[@@toPrimitive]` — installed as real symbol-keyed
//! entries when the Symbol / Date prototype singleton is minted.
//!
//! Before this they were tag special cases inside ToPrimitive, so the
//! property did not exist: `Symbol.toPrimitive in Symbol.prototype`
//! was false, and deleting or redefining it changed nothing. As real
//! entries they read, reflect, delete and redefine like the spec's
//! data properties, and ToPrimitive's GetMethod finds whatever is
//! there now (`to_primitive::exotic_to_primitive`).
//!
//! A child of `closure_proto` for the same reach into the mint
//! plumbing `iter_proto` uses.

use core::ffi::c_void;

use super::iter_proto::mint_symbol_method_cell;
use super::{__torajs_dynobj_define_plain, ANY_HEAP};
use crate::method_value::symbol_static;
use crate::nanbox::{
    AnyValue, VALUE_UNDEFINED, as_void_ptr, box_pointer, is_cell, is_short_str, short_str_bytes,
    short_str_len,
};

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

/// Alphabetical well-known index of `toPrimitive`.
const WK_TO_PRIMITIVE: i64 = 12;
/// Builtin-proto slots of the two prototypes that own the method.
const SYMBOL_PROTO_IDX: i64 = 5;
const DATE_PROTO_IDX: i64 = 8;
/// {W:0, E:0, C:1} — both clauses define the property non-writable.
const TO_PRIMITIVE_ENTRY_FLAGS: u64 = (1 << 6) | (1 << 5) | (1 << 4) | (1 << 3) | (1 << 2);

/// Argument `i` of a receiver-first call (`argv[0]` is `this`).
unsafe fn arg(argv: *const u64, argc: i64, i: i64) -> AnyValue {
    if argc > i && !argv.is_null() {
        unsafe { *argv.add(i as usize) }
    } else {
        VALUE_UNDEFINED
    }
}

/// Whether `v` is the string `name` (short immediate or Str cell).
unsafe fn string_is(v: AnyValue, name: &[u8]) -> bool {
    if is_short_str(v) {
        let len = short_str_len(v) as usize;
        return len == name.len() && short_str_bytes(v)[..len] == *name;
    }
    if !is_cell(v) {
        return false;
    }
    let p = as_void_ptr(v);
    // SAFETY: a cell box points at a live header; the tag word is +4.
    let tag = unsafe { (p.cast::<u8>().add(4) as *const u16).read() };
    tag == torajs_rc::Tag::Str as u16 && unsafe { crate::prop_has::key_is(p, name) }
}

/// §20.4.3.5 — thisSymbolValue(this); the hint is ignored.
unsafe extern "C" fn symbol_to_primitive_entry(
    _env: *mut c_void,
    argv: *const u64,
    argc: i64,
) -> u64 {
    let recv = unsafe { arg(argv, argc, 0) };
    match unsafe { crate::method_call_cell::this_symbol_value(recv) } {
        Some(sym) => {
            unsafe { torajs_rc::__torajs_rc_inc(sym) };
            box_pointer(sym.cast())
        }
        None => {
            unsafe {
                __torajs_throw_type_error(
                    c"Symbol.prototype[Symbol.toPrimitive] requires that |this| be a Symbol"
                        .as_ptr(),
                )
            };
            VALUE_UNDEFINED
        }
    }
}

/// §21.4.4.45 — `this` must be an object; hint "string" / "default"
/// is string order, "number" is number order, anything else throws.
unsafe extern "C" fn date_to_primitive_entry(
    _env: *mut c_void,
    argv: *const u64,
    argc: i64,
) -> u64 {
    let recv = unsafe { arg(argv, argc, 0) };
    let hint = unsafe { arg(argv, argc, 1) };
    if !crate::to_primitive::is_object_value(recv) {
        unsafe {
            __torajs_throw_type_error(
                c"Date.prototype[Symbol.toPrimitive] requires that |this| be an Object".as_ptr(),
            )
        };
        return VALUE_UNDEFINED;
    }
    let hint_string = if unsafe { string_is(hint, b"string") || string_is(hint, b"default") } {
        true
    } else if unsafe { string_is(hint, b"number") } {
        false
    } else {
        unsafe { __torajs_throw_type_error(c"invalid hint for Symbol.toPrimitive".as_ptr()) };
        return VALUE_UNDEFINED;
    };
    // SAFETY: an object value is a live cell.
    unsafe { crate::to_primitive::ordinary_to_primitive(as_void_ptr(recv), hint_string) }
        .unwrap_or(VALUE_UNDEFINED)
}

/// Builtin-proto mint face: the Symbol or Date prototype's own
/// `[Symbol.toPrimitive]` entry. Other slots are a no-op.
///
/// # Safety
/// `proto` is the freshly allocated prototype dynobj (non-null).
pub(crate) unsafe fn install(proto: *mut c_void, idx: i64) {
    let entry: unsafe extern "C" fn(*mut c_void, *const u64, i64) -> u64 = match idx {
        SYMBOL_PROTO_IDX => symbol_to_primitive_entry,
        DATE_PROTO_IDX => date_to_primitive_entry,
        _ => return,
    };
    let key = symbol_static::well_known_singleton(WK_TO_PRIMITIVE) as *mut c_void;
    if key.is_null() {
        return;
    }
    let cell = unsafe { mint_symbol_method_cell(entry, b"[Symbol.toPrimitive]", 1) };
    let mut slot = proto;
    unsafe {
        __torajs_dynobj_define_plain(
            &mut slot,
            key,
            ANY_HEAP,
            cell as u64,
            TO_PRIMITIVE_ENTRY_FLAGS,
        )
    };
}
