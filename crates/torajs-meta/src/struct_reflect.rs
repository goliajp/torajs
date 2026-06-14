//! W-J Phase B — struct-cell arm for `Object.getOwnPropertyDescriptor`.
//!
//! `reflect.rs`'s `__torajs_anyv_get_property_descriptor` handles the
//! `Tag::DynObj` (dynamic-property object) cell. Static-layout class /
//! anonymous-struct instances carry `Tag::Obj` and store their fields
//! at fixed byte offsets instead of in a hash table, so they need a
//! different read path: look the field up in the toolchain-emitted
//! `__torajs_class_layouts` table (via the `torajs-structmeta` Phase A4
//! helpers) and read the raw 8-byte slot directly.
//!
//! This is the first **user-visible** unlock of the W-J substrate trunk:
//! `Object.getOwnPropertyDescriptor({a: 1}, "a")` now returns the full
//! `{ value, writable, enumerable, configurable }` descriptor instead of
//! `undefined`.
//!
//! Struct fields are ordinary data properties — ECMA-262 §10.4.x makes
//! them `writable` / `enumerable` / `configurable` all `true` (a
//! `defineProperty` that demotes one of these moves the object onto the
//! dynobj path, which is orthogonal to this read arm).
//!
//! ## Key encoding caveat (MVP)
//!
//! Field names in the metadata are the source UTF-8 bytes; the lookup
//! key is a `Str` whose payload is Latin-1 for ASCII identifiers. The
//! two coincide for ASCII property names (the overwhelming majority).
//! A non-ASCII (UTF-16 `Str`) key would not byte-match a UTF-8 field
//! name — a precision follow-up, not a correctness regression for the
//! cases this unlocks.

use core::ffi::c_void;

unsafe extern "C" {
    // torajs-structmeta (W-J Phase A4) — read-side over __torajs_class_layouts.
    fn __torajs_struct_layout_lookup(class_tag: u32) -> *const c_void;
    fn __torajs_struct_field_find(layout: *const c_void, name: *const u8, name_len: u32) -> u32;
    fn __torajs_struct_field_info(layout: *const c_void, idx: u32) -> FieldInfo;

    // torajs-anyvalue — decode a NaN-box AnyValue back into its
    // (tag, value) pair so an `any`-typed field slot can be re-stored
    // through `__torajs_dynobj_set`'s pair interface.
    fn __torajs_anyv_unbox_tag(v: u64) -> i64;
    fn __torajs_anyv_unbox_value(v: u64) -> i64;

    // torajs-dynobj + sibling helpers — same surface reflect.rs uses to
    // build the descriptor object.
    fn __torajs_dynobj_alloc() -> *mut c_void;
    fn __torajs_dynobj_set(dst: *mut *mut c_void, key: *const u8, tag: u64, value: u64);
    fn __torajs_str_alloc_pooled(len: u64) -> *mut u8;
    fn __torajs_str_drop(s: *mut u8);
    fn __torajs_rc_inc(p: *mut c_void);
}

/// Field byte-offset + coarse type tag — mirrors
/// `torajs-structmeta::FieldInfo` (returned by value across the FFI).
#[repr(C)]
struct FieldInfo {
    field_byte_offset: u32,
    type_tag: u8,
}

// AnySlotTag values (mirror torajs-anyvalue::AnySlotTag — the same i64
// wire tags `box_from_pair` decodes).
const ANY_BOOL: u64 = 1;
const ANY_I64: u64 = 2;
const ANY_F64: u64 = 3;
const ANY_HEAP: u64 = 4;

/// `undefined` NaN-box immediate (mirror reflect.rs).
const VALUE_UNDEFINED_IMM: u64 = 0x0A;

/// Byte offset of the `class_tag` u32 inside a `Tag::Obj` instance —
/// right after the 8-byte universal heap header (mirror
/// torajs-cycle::layout::OBJ_CLASS_TAG_OFF).
const OBJ_CLASS_TAG_OFF: usize = 8;

/// `Str` layout: `[header:8][length u32 @8][_pad:4][bytes @16]`
/// (mirror torajs-str::layout STR_LEN_OFF / STR_DATA_OFF).
const STR_LEN_OFF: usize = 8;
const STR_DATA_OFF: usize = 16;

/// Build a pooled `Str` key holding `name`'s bytes (mirror reflect.rs's
/// private `alloc_str_key`).
#[inline]
unsafe fn alloc_str_key(name: &[u8]) -> *mut u8 {
    let s = unsafe { __torajs_str_alloc_pooled(name.len() as u64) };
    if !name.is_empty() {
        unsafe { core::ptr::copy_nonoverlapping(name.as_ptr(), s.add(STR_DATA_OFF), name.len()) };
    }
    s
}

/// Map a struct field's coarse `type_tag` (see
/// `ssa::field_type_tag_of`) + its raw 8-byte slot to an AnyValue
/// `(tag, value)` pair for `__torajs_dynobj_set`.
///
/// - `0` (`Any`): the slot is already a NaN-box AnyValue — decode it.
/// - `1` (`I32`/`I64`): integer payload.
/// - `2` (`F64`): IEEE-754 bits.
/// - `3` (`Bool`): `0` / `1`.
/// - `4..=21` (`Str` / `Arr` / `Obj` / `Map` / ... heap cells): raw
///   heap pointer.
#[inline]
pub(crate) unsafe fn field_slot_to_pair(type_tag: u8, raw: u64) -> (u64, u64) {
    match type_tag {
        0 => {
            let t = unsafe { __torajs_anyv_unbox_tag(raw) } as u64;
            let v = unsafe { __torajs_anyv_unbox_value(raw) } as u64;
            (t, v)
        }
        1 => (ANY_I64, raw),
        2 => (ANY_F64, raw),
        3 => (ANY_BOOL, raw),
        _ => (ANY_HEAP, raw),
    }
}

/// `Object.getOwnPropertyDescriptor` arm for a `Tag::Obj` struct cell.
/// Returns the 4-key descriptor as a NaN-box AnyValue cell, or
/// `undefined` when the class has no layout (anonymous struct interned
/// too late — `class_tag` 0 / out of range) or the key is not a field.
///
/// # Safety
/// `cell` is a live `Tag::Obj` heap pointer (caller checked the header
/// tag); `key` is a live `Str` pointer (caller checked non-NULL).
pub(crate) unsafe fn struct_cell_descriptor(cell: *const c_void, key: *const c_void) -> u64 {
    // class_tag (u32 @ +8) → layout entry. 0 / out-of-range → no layout.
    let class_tag = unsafe {
        cell.cast::<u8>()
            .add(OBJ_CLASS_TAG_OFF)
            .cast::<u32>()
            .read()
    };
    let layout = unsafe { __torajs_struct_layout_lookup(class_tag) };
    if layout.is_null() {
        return VALUE_UNDEFINED_IMM;
    }

    // Resolve the key to a field index.
    let k = key as *const u8;
    let key_len = unsafe { k.add(STR_LEN_OFF).cast::<u32>().read() };
    let key_bytes = unsafe { k.add(STR_DATA_OFF) };
    let idx = unsafe { __torajs_struct_field_find(layout, key_bytes, key_len) };
    if idx == u32::MAX {
        return VALUE_UNDEFINED_IMM;
    }

    // Read the raw 8-byte slot at the field's byte offset, box per type.
    let info = unsafe { __torajs_struct_field_info(layout, idx) };
    let raw = unsafe {
        cell.cast::<u8>()
            .add(info.field_byte_offset as usize)
            .cast::<u64>()
            .read()
    };
    let (v_tag, v_val) = unsafe { field_slot_to_pair(info.type_tag, raw) };
    // The descriptor owns its own ref to a heap value.
    if v_tag == ANY_HEAP && v_val != 0 {
        unsafe { __torajs_rc_inc(v_val as *mut c_void) };
    }

    // Build `{ value, writable: true, enumerable: true, configurable: true }`.
    let mut desc = unsafe { __torajs_dynobj_alloc() };
    let entries: [(&[u8], u64, u64); 4] = [
        (b"value", v_tag, v_val),
        (b"writable", ANY_BOOL, 1),
        (b"enumerable", ANY_BOOL, 1),
        (b"configurable", ANY_BOOL, 1),
    ];
    for &(name, t, val) in entries.iter() {
        let kk = unsafe { alloc_str_key(name) };
        unsafe { __torajs_dynobj_set(&mut desc, kk, t, val) };
        unsafe { __torajs_str_drop(kk) };
    }
    desc as u64
}
