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
    /// Accessor slot standing for a plain property name (`kind` 0 =
    /// getter, 1 = setter) — RFC 20260714-objlit-accessor.
    fn __torajs_struct_accessor_find(
        layout: *const c_void,
        name: *const u8,
        name_len: u32,
        kind: u8,
    ) -> u32;
    fn __torajs_struct_field_info(layout: *const c_void, idx: u32) -> FieldInfo;

    // torajs-anyvalue — decode a NaN-box AnyValue back into its
    // (tag, value) pair so an `any`-typed field slot can be re-stored
    // through `__torajs_dynobj_set`'s pair interface.
    fn __torajs_anyv_unbox_tag(v: u64) -> i64;
    fn __torajs_anyv_unbox_value(v: u64) -> i64;

    // torajs-rc — the descriptor's heap value owns a fresh share.
    fn __torajs_rc_inc(p: *mut c_void);

    // torajs-str — undefined sentinel identity probe (RFC 20260710
    // C1): a Str/Substr slot holding the immortal sentinel cell means
    // JS `undefined`, not a 9-char string.
    fn __torajs_str_is_undef(p: *const u8) -> i64;

    // torajs-rc — generic undefined oddball probe (RFC 20260710
    // C2b): a refcounted pointer slot (Obj / Arr / Closure) holding
    // the Tag::Undefined cell means JS `undefined`.
    fn __torajs_is_undef_cell(p: *const u8) -> i64;
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
const ANY_UNDEF: u64 = 5;

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
        _ => {
            // RFC 20260710 C1/C2b — a slot holding either undefined
            // sentinel (the Str/Substr cell or the generic
            // Tag::Undefined cell) reads back as JS `undefined`
            // (struct print / descriptors would otherwise render the
            // Str cell's "undefined" payload as a quoted string, or
            // walk the bare header as a live cell).
            if raw != 0
                && (unsafe { __torajs_str_is_undef(raw as *const u8) } != 0
                    || unsafe { __torajs_is_undef_cell(raw as *const u8) } != 0)
            {
                (ANY_UNDEF, 0)
            } else {
                (ANY_HEAP, raw)
            }
        }
    }
}

/// Accessor half spellings for [`__torajs_struct_accessor_find`].
pub(crate) const ACC_GETTER: u8 = 0;
pub(crate) const ACC_SETTER: u8 = 1;

/// Object-literal accessor slot prefixes — the single mirror of
/// torajs-core's `check_type_of_object_lit::accessor_slot`. Every
/// reflection consumer in this crate (keys / entries / print / gOPD)
/// resolves slot names through [`accessor_slot_name`], so the spelling
/// lives in exactly one place.
const ACCESSOR_PREFIXES: [(&[u8], u8); 2] =
    [(b"__getter_", ACC_GETTER), (b"__setter_", ACC_SETTER)];

/// Split a layout slot name into `(kind, plain-name)` when it is an
/// accessor slot (`__getter_v` → `(ACC_GETTER, "v")`); a data field
/// answers `None`.
///
/// # Safety
/// `ptr` is NULL or points at `len` readable bytes.
pub(crate) unsafe fn accessor_slot_name(
    ptr: *const u8,
    len: usize,
) -> Option<(u8, *const u8, usize)> {
    if ptr.is_null() {
        return None;
    }
    let bytes = unsafe { core::slice::from_raw_parts(ptr, len) };
    for (prefix, kind) in ACCESSOR_PREFIXES {
        if bytes.len() > prefix.len() && &bytes[..prefix.len()] == prefix {
            return Some((kind, unsafe { ptr.add(prefix.len()) }, len - prefix.len()));
        }
    }
    None
}

/// The own-property key a layout slot enumerates under: an accessor
/// slot answers the property it stands for, a data field answers
/// itself.
///
/// # Safety
/// `ptr` is NULL or points at `len` readable bytes.
pub(crate) unsafe fn slot_key(ptr: *const u8, len: usize) -> (*const u8, usize) {
    match unsafe { accessor_slot_name(ptr, len) } {
        Some((_, p, l)) => (p, l),
        None => (ptr, len),
    }
}

/// The closure sitting in an accessor slot, as a `(tag, value)` pair
/// the descriptor can own: `(ANY_UNDEF, 0)` when the class has no such
/// half. The descriptor takes a fresh reference — the struct keeps its
/// own share of the slot.
///
/// # Safety
/// `cell` is a live `Tag::Obj` heap pointer; `layout` is its live
/// layout entry; `prop` points at `prop_len` readable bytes.
unsafe fn accessor_half(
    cell: *const c_void,
    layout: *const c_void,
    prop: *const u8,
    prop_len: u32,
    kind: u8,
) -> (u64, u64) {
    let idx = unsafe { __torajs_struct_accessor_find(layout, prop, prop_len, kind) };
    if idx == u32::MAX {
        return (ANY_UNDEF, 0);
    }
    let info = unsafe { __torajs_struct_field_info(layout, idx) };
    let raw = unsafe {
        cell.cast::<u8>()
            .add(info.field_byte_offset as usize)
            .cast::<u64>()
            .read()
    };
    if raw == 0 {
        return (ANY_UNDEF, 0);
    }
    unsafe { __torajs_rc_inc(raw as *mut c_void) };
    (ANY_HEAP, raw)
}

/// `Object.getOwnPropertyDescriptor` for a property the layout carries
/// as an accessor slot (RFC 20260714-objlit-accessor). Answers
/// `{ get, set, enumerable: true, configurable: true }` — an
/// object-literal accessor is an ordinary own property (§10.4), so
/// both flags are set, exactly like the data-field arm. `undefined`
/// when neither half exists (the key is simply not a member).
///
/// # Safety
/// `cell` is a live `Tag::Obj` heap pointer; `layout` is its live
/// layout entry; `prop` points at `prop_len` readable bytes.
unsafe fn struct_accessor_descriptor(
    cell: *const c_void,
    layout: *const c_void,
    prop: *const u8,
    prop_len: u32,
) -> u64 {
    let (get_t, get_v) = unsafe { accessor_half(cell, layout, prop, prop_len, ACC_GETTER) };
    let (set_t, set_v) = unsafe { accessor_half(cell, layout, prop, prop_len, ACC_SETTER) };
    if get_t == ANY_UNDEF && set_t == ANY_UNDEF {
        return VALUE_UNDEFINED_IMM;
    }
    unsafe { crate::reflect::build_accessor_descriptor(get_t, get_v, set_t, set_v, 1, 1) }
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
        // RFC 20260714-objlit-accessor — the plain name never matches
        // an accessor, which the layout carries under a synthetic slot
        // (`__getter_v` / `__setter_v`). Answer the accessor
        // descriptor before conceding `undefined`.
        return unsafe { struct_accessor_descriptor(cell, layout, key_bytes, key_len) };
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
    unsafe { crate::reflect::build_data_descriptor(v_tag, v_val, 1, 1, 1) }
}
