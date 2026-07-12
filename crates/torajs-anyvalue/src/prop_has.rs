//! `__torajs_any_prop_has` — own-property presence probe on an `any`
//! receiver (chunk B3, RFC 20260711 for-in).
//!
//! Primary consumer: the for-in mid-loop-delete guard — ES §14.7.5
//! EnumerateObjectProperties requires that properties deleted during
//! enumeration are NOT visited, so the lowering re-checks each key
//! before running the body. Also the substrate the hasOwnProperty /
//! propertyIsEnumerable port (RFC chunk D) will consume.
//!
//! Presence — not value truthiness: an entry whose stored value is
//! `undefined` still answers 1 (mirrors `"k" in o`), which is why the
//! dynobj arms route through `__torajs_dynobj_has` instead of the
//! nullish-tag probe `any_method_probe` uses.
//!
//! Per-receiver dispatch mirrors `prop_delete`:
//!
//! - null / undefined receiver → catchable TypeError, answers 0
//!   (hasOwnProperty ToObject shape; the for-in guard never sends
//!   these — a null source enumerates nothing upstream).
//! - `Tag::DynObj` → `__torajs_dynobj_has`.
//! - `Tag::Arr` → canonical index in `[0, len)` or `"length"` → 1;
//!   otherwise the expando props dynobj.
//! - `Tag::Closure` → expando props dynobj; `"length"` / `"name"`
//!   answer 1 per §20.2.4 (own non-enumerable in spec — tr carries
//!   them virtually).
//! - `Tag::Obj` (struct cell) → class-layout field probe.
//! - `Tag::Str` cell / ShortStr imm → index in `[0, len)` or
//!   `"length"` → 1 (§22.1.5.2).
//! - every other receiver → 0 (primitive ToObject wrappers carry no
//!   own string-keyed properties).

use core::ffi::c_void;

use torajs_rc::Tag;

use crate::member_get::{closure_props, header_flag, recv_cell};
use crate::nanbox::{AnyValue, is_null, is_short_str, is_undefined};

unsafe extern "C" {
    /// torajs-dynobj — own-entry presence (1 = live entry).
    fn __torajs_dynobj_has(obj: *const c_void, key: *const c_void) -> i32;
    /// torajs-dynobj — packed W/E/C data-attribute flags
    /// (bit 1 = enumerable); answers 0 for an absent key, which is
    /// exactly the propertyIsEnumerable miss semantics.
    fn __torajs_dynobj_get_flags(obj: *const c_void, key: *const c_void) -> u64;
    /// torajs-structmeta — class-layout lookup + field probe.
    fn __torajs_struct_layout_lookup(class_tag: u32) -> *const c_void;
    fn __torajs_struct_field_find(layout: *const c_void, name: *const u8, name_len: u32) -> u32;
    /// torajs-throw — record a pending catchable TypeError.
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

/// Layout mirrors (see `member_get`): struct `class_tag` u32 at +8;
/// Str len u32 at +8, bytes at +16; Arr len u64 at +8, inline
/// props-dynobj slot at +24 (`torajs_arr::layout::ARR_PROPS_OFF`);
/// Closure props-dynobj slot at +24.
const OBJ_CLASS_TAG_OFF: usize = 8;
const STR_LEN_OFF: usize = 8;
const STR_DATA_OFF: usize = 16;
const ARR_LEN_OFF: usize = 8;
const ARR_PROPS_OFF: usize = 24;

/// Decode the key Str cell to `(bytes, len)`.
#[inline]
unsafe fn key_bytes(key: *const c_void) -> (*const u8, u32) {
    let len = unsafe { key.cast::<u8>().add(STR_LEN_OFF).cast::<u32>().read() };
    (unsafe { key.cast::<u8>().add(STR_DATA_OFF) }, len)
}

/// Parse a canonical array-index key (`"0"`, `"12"` — all digits, no
/// leading zero except `"0"` itself). `None` for anything else.
unsafe fn canonical_index(key: *const c_void) -> Option<u64> {
    let (bytes, len) = unsafe { key_bytes(key) };
    if len == 0 || len > 20 {
        return None;
    }
    let s = unsafe { core::slice::from_raw_parts(bytes, len as usize) };
    if !s.iter().all(u8::is_ascii_digit) || (len > 1 && s[0] == b'0') {
        return None;
    }
    let mut v: u64 = 0;
    for &b in s {
        v = v.checked_mul(10)?.checked_add((b - b'0') as u64)?;
    }
    Some(v)
}

/// `true` iff the key spells exactly `name`.
pub(crate) unsafe fn key_is(key: *const c_void, name: &[u8]) -> bool {
    let (bytes, len) = unsafe { key_bytes(key) };
    len as usize == name.len()
        && unsafe { core::slice::from_raw_parts(bytes, len as usize) } == name
}

/// See module doc. `key` is a live Str cell (the lowering interns
/// static names and materializes dynamic string keys before the
/// call).
///
/// # Safety
/// Cell receivers are valid heap pointers; `key` is a live Str cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_prop_has(recv: AnyValue, key: *const c_void) -> i64 {
    if is_null(recv) || is_undefined(recv) {
        unsafe {
            __torajs_throw_type_error(c"cannot read properties of null or undefined".as_ptr());
        }
        return 0;
    }
    if is_short_str(recv) {
        // ShortStr imm — len lives in bits 47..40 (SSO layout).
        let len = (recv >> 40) & 0xFF;
        return unsafe { str_index_has(len, key) };
    }
    match recv_cell(recv) {
        Some((ptr, t)) if t == Tag::DynObj as u16 => {
            if unsafe { __torajs_dynobj_has(ptr, key) } != 0 {
                return 1;
            }
            // Builtin `<Ctor>.prototype` singleton — an interned
            // builtin method is the prototype's own property per
            // spec even though it lives in the method-cell table,
            // not the dynobj (RFC 20260712 chunk 1; the
            // delete-tombstone shading is chunk 3).
            unsafe { builtin_proto_method_own(ptr, key) }
        }
        Some((ptr, t)) if t == Tag::Arr as u16 => {
            let len = unsafe { ptr.cast::<u8>().add(ARR_LEN_OFF).cast::<u64>().read() };
            if let Some(i) = unsafe { canonical_index(key) }
                && i < len
            {
                return 1;
            }
            if unsafe { key_is(key, b"length") } {
                return 1;
            }
            let props = unsafe {
                ptr.cast::<u8>()
                    .add(ARR_PROPS_OFF)
                    .cast::<*const c_void>()
                    .read()
            };
            if props.is_null() {
                0
            } else {
                unsafe { __torajs_dynobj_has(props, key) as i64 }
            }
        }
        Some((ptr, t)) if t == Tag::Closure as u16 => {
            // chunk C — a tombstoned virtual prop is gone until an
            // expando write recreates it (probed below).
            if unsafe { key_is(key, b"length") }
                && !unsafe { header_flag(ptr, torajs_rc::FLAG_FN_LENGTH_DELETED) }
            {
                return 1;
            }
            if unsafe { key_is(key, b"name") }
                && !unsafe { header_flag(ptr, torajs_rc::FLAG_FN_NAME_DELETED) }
            {
                return 1;
            }
            let props = unsafe { closure_props(ptr) };
            if props.is_null() {
                0
            } else {
                unsafe { __torajs_dynobj_has(props, key) as i64 }
            }
        }
        Some((ptr, t)) if t == Tag::Obj as u16 => {
            let class_tag = unsafe { ptr.cast::<u8>().add(OBJ_CLASS_TAG_OFF).cast::<u32>().read() };
            let layout = unsafe { __torajs_struct_layout_lookup(class_tag) };
            if layout.is_null() {
                return 0;
            }
            let (name_bytes, name_len) = unsafe { key_bytes(key) };
            (unsafe { __torajs_struct_field_find(layout, name_bytes, name_len) } != u32::MAX) as i64
        }
        Some((ptr, t)) if t == Tag::Str as u16 => {
            let len = unsafe { ptr.cast::<u8>().add(STR_LEN_OFF).cast::<u32>().read() } as u64;
            unsafe { str_index_has(len, key) }
        }
        _ => 0,
    }
}

/// Own-property probe for a dynobj that turns out to be a builtin
/// `<Ctor>.prototype` singleton: the interned method families count
/// as own entries (`proto_tag_owns` — the universal probes stay
/// Object.prototype-only). Answers 0 for every ordinary dynobj.
unsafe fn builtin_proto_method_own(ptr: *const c_void, key: *const c_void) -> i64 {
    let tag = unsafe { torajs_rc::builtin_proto::__torajs_builtin_proto_tag_of(ptr) };
    if tag < 0 {
        return 0;
    }
    let mid = unsafe { crate::method_value::key_method_id(key) };
    if mid == torajs_rc::ANY_METHOD_UNKNOWN {
        // The Map/Set `size` accessor is an own property too — it
        // deliberately doesn't intern (C2-size), so probe it here
        // behind the same tombstone gate.
        if let Some(amid) = unsafe { crate::method_support::proto_tag_accessor_mid(tag, key) } {
            let deleted =
                unsafe { torajs_rc::builtin_proto::__torajs_builtin_proto_is_deleted(tag, amid) };
            return (deleted == 0) as i64;
        }
        return 0;
    }
    crate::method_support::proto_tag_owns(tag, mid) as i64
}

/// Shared Str-receiver arm: index in `[0, len)` or `"length"` → 1.
unsafe fn str_index_has(len: u64, key: *const c_void) -> i64 {
    if let Some(i) = unsafe { canonical_index(key) }
        && i < len
    {
        return 1;
    }
    unsafe { key_is(key, b"length") as i64 }
}

/// `Object.prototype.propertyIsEnumerable` substrate (chunk D-1,
/// RFC 20260711): own AND enumerable. Mirrors
/// [`__torajs_any_prop_has`]'s dispatch with the enumerable-flag
/// filter applied where flags exist:
///
/// - DynObj / expando entries → packed-flags bit 1 (absent key
///   answers 0 flags — the miss and the non-enumerable case
///   coincide, matching §20.1.4.5).
/// - Arr / Str index keys and struct fields → enumerable when
///   present (tr data slots carry no non-enumerable state).
/// - `length` / `name` virtual props → 0 (spec non-enumerable).
/// - primitives → 0; null / undefined → catchable TypeError.
///
/// # Safety
/// Cell receivers are valid heap pointers; `key` is a live Str cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_prop_enumerable(recv: AnyValue, key: *const c_void) -> i64 {
    if is_null(recv) || is_undefined(recv) {
        unsafe {
            __torajs_throw_type_error(c"cannot read properties of null or undefined".as_ptr());
        }
        return 0;
    }
    if is_short_str(recv) {
        let len = (recv >> 40) & 0xFF;
        return unsafe { str_index_enumerable(len, key) };
    }
    match recv_cell(recv) {
        Some((ptr, t)) if t == Tag::DynObj as u16 => {
            ((unsafe { __torajs_dynobj_get_flags(ptr, key) } & 0x2) != 0) as i64
        }
        Some((ptr, t)) if t == Tag::Arr as u16 => {
            let len = unsafe { ptr.cast::<u8>().add(ARR_LEN_OFF).cast::<u64>().read() };
            if let Some(i) = unsafe { canonical_index(key) }
                && i < len
            {
                return 1;
            }
            let props = unsafe {
                ptr.cast::<u8>()
                    .add(ARR_PROPS_OFF)
                    .cast::<*const c_void>()
                    .read()
            };
            if props.is_null() {
                0
            } else {
                ((unsafe { __torajs_dynobj_get_flags(props, key) } & 0x2) != 0) as i64
            }
        }
        Some((ptr, t)) if t == Tag::Closure as u16 => {
            let props = unsafe { closure_props(ptr) };
            if props.is_null() {
                0
            } else {
                ((unsafe { __torajs_dynobj_get_flags(props, key) } & 0x2) != 0) as i64
            }
        }
        Some((ptr, t)) if t == Tag::Obj as u16 => {
            let class_tag = unsafe { ptr.cast::<u8>().add(OBJ_CLASS_TAG_OFF).cast::<u32>().read() };
            let layout = unsafe { __torajs_struct_layout_lookup(class_tag) };
            if layout.is_null() {
                return 0;
            }
            let (name_bytes, name_len) = unsafe { key_bytes(key) };
            (unsafe { __torajs_struct_field_find(layout, name_bytes, name_len) } != u32::MAX) as i64
        }
        Some((ptr, t)) if t == Tag::Str as u16 => {
            let len = unsafe { ptr.cast::<u8>().add(STR_LEN_OFF).cast::<u32>().read() } as u64;
            unsafe { str_index_enumerable(len, key) }
        }
        _ => 0,
    }
}

/// Str-receiver enumerable arm: index chars are enumerable,
/// `length` is not (§22.1.5.1).
unsafe fn str_index_enumerable(len: u64, key: *const c_void) -> i64 {
    if let Some(i) = unsafe { canonical_index(key) }
        && i < len
    {
        return 1;
    }
    0
}
