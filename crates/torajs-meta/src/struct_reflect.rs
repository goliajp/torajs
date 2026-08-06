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
    // through `__torajs_dynobj_set`'s pair interface. The owned
    // variant hands the caller its own stake (cell +1 / ShortStr
    // materialized at rc=1); the pair encoder re-boxes typed slots.
    fn __torajs_anyv_unbox_tag(v: u64) -> i64;
    fn __torajs_anyv_unbox_value_owned(v: u64) -> i64;
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;

    // torajs-rc — the accessor descriptor's closure halves own a
    // fresh share each.
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
/// `ssa::field_type_tag_of`) + its raw 8-byte slot to a BORROWED
/// NaN-box AnyValue (the struct keeps every heap stake; an Any slot
/// passes its box through untouched — no ShortStr materialization).
///
/// - `0` (`Any`): the slot is already a NaN-box AnyValue — verbatim.
/// - `1` (`I32`/`I64`): integer payload.
/// - `2` (`F64`): IEEE-754 bits.
/// - `3` (`Bool`): `0` / `1`.
/// - `4..=21` (`Str` / `Arr` / `Obj` / `Map` / ... heap cells): raw
///   heap pointer, undefined sentinels normalized (RFC 20260710
///   C1/C2b — a slot holding either sentinel cell reads back as JS
///   `undefined`, not a 9-char string / a bare live cell).
#[inline]
pub(crate) unsafe fn field_slot_to_anyv_borrowed(type_tag: u8, raw: u64) -> u64 {
    match type_tag {
        0 => raw,
        1 => unsafe { __torajs_anyv_box_from_pair(ANY_I64 as i64, raw as i64) },
        2 => unsafe { __torajs_anyv_box_from_pair(ANY_F64 as i64, raw as i64) },
        3 => unsafe { __torajs_anyv_box_from_pair(ANY_BOOL as i64, raw as i64) },
        _ => {
            if raw != 0
                && (unsafe { __torajs_str_is_undef(raw as *const u8) } != 0
                    || unsafe { __torajs_is_undef_cell(raw as *const u8) } != 0)
            {
                VALUE_UNDEFINED_IMM
            } else {
                // raw == 0 encodes as VALUE_NULL through the pair
                // encoder's Heap arm — same normalization the old
                // borrow-pair consumers produced downstream.
                unsafe { __torajs_anyv_box_from_pair(ANY_HEAP as i64, raw as i64) }
            }
        }
    }
}

/// [`field_slot_to_anyv_borrowed`] decoded to an OWNED `(tag, value)`
/// pair — the caller receives its own stake on a heap payload (cell
/// +1; an Any slot's ShortStr materializes at rc=1, the
/// materialization itself being the stake). Consumers hand the pair
/// straight to an adopting sink (`__torajs_arr_push_any` /
/// `__torajs_dynobj_set` / `build_data_descriptor`) with NO extra
/// inc — the pre-fix borrow-pair + caller-side `rc_inc` left a
/// ShortStr slot's materialized Str at rc=2 with one reclaiming
/// drop (32B leaked per `Object.values` / gOPD / struct print read).
#[inline]
pub(crate) unsafe fn field_slot_to_pair_owned(type_tag: u8, raw: u64) -> (u64, u64) {
    let anyv = unsafe { field_slot_to_anyv_borrowed(type_tag, raw) };
    let tag = unsafe { __torajs_anyv_unbox_tag(anyv) } as u64;
    let val = unsafe { __torajs_anyv_unbox_value_owned(anyv) } as u64;
    (tag, val)
}

/// Owned NaN-box read of a struct cell's own DATA field `name`, or
/// `None` when the class layout has no such field (accessor slots
/// deliberately not probed — callers want the data shadow only).
/// Simulation-slot key separation: `o.__proto__` on a struct-typed
/// literal carrying an own `__proto__` data field (shorthand
/// `{__proto__}`) must answer the field per §10.1.8.1 own-first,
/// not the [[Prototype]].
pub(crate) unsafe fn struct_own_field_anyv(cell: *const c_void, name: &[u8]) -> Option<u64> {
    let class_tag = unsafe {
        cell.cast::<u8>()
            .add(OBJ_CLASS_TAG_OFF)
            .cast::<u32>()
            .read()
    };
    let layout = unsafe { __torajs_struct_layout_lookup(class_tag) };
    if layout.is_null() {
        return None;
    }
    let idx = unsafe { __torajs_struct_field_find(layout, name.as_ptr(), name.len() as u32) };
    if idx == u32::MAX {
        return None;
    }
    let info = unsafe { __torajs_struct_field_info(layout, idx) };
    let raw = unsafe {
        cell.cast::<u8>()
            .add(info.field_byte_offset as usize)
            .cast::<u64>()
            .read()
    };
    let (t, v) = unsafe { field_slot_to_pair_owned(info.type_tag, raw) };
    Some(unsafe { __torajs_anyv_box_from_pair(t as i64, v as i64) })
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
/// `{ get, set, enumerable, configurable }` — an object-literal
/// accessor is an ordinary own property (§10.4), so both flags start
/// out set, exactly like the data-field arm, and move for the same two
/// reasons: `Object.freeze` / `seal`, and a `defineProperty` that left
/// a sidecar. `undefined` when neither half exists (the key is simply
/// not a member).
///
/// # Safety
/// `cell` is a live `Tag::Obj` heap pointer; `layout` is its live
/// layout entry; `prop` points at `prop_len` readable bytes; `key` is
/// the live key cell spelling the same name.
unsafe fn struct_accessor_descriptor(
    cell: *const c_void,
    layout: *const c_void,
    prop: *const u8,
    prop_len: u32,
    key: *const c_void,
) -> u64 {
    let (get_t, get_v) = unsafe { accessor_half(cell, layout, prop, prop_len, ACC_GETTER) };
    let (set_t, set_v) = unsafe { accessor_half(cell, layout, prop, prop_len, ACC_SETTER) };
    if get_t == ANY_UNDEF && set_t == ANY_UNDEF {
        return VALUE_UNDEFINED_IMM;
    }
    let attrs = unsafe { crate::struct_field_attrs::declared_member_attrs(cell, key) };
    unsafe {
        crate::reflect::build_accessor_descriptor(
            get_t,
            get_v,
            set_t,
            set_v,
            u64::from(attrs & crate::struct_field_attrs::BUCKET_FLAG_ENUMERABLE != 0),
            u64::from(attrs & crate::struct_field_attrs::BUCKET_FLAG_CONFIGURABLE != 0),
        )
    }
}

/// The descriptor for an EXPANDO entry — an own property the layout
/// metadata cannot mention because it was written at runtime (RFC
/// 20260714-struct-dynamic-props; `err.cause` is one the injected
/// Error ctor installs). The entries live in an ordinary dynobj at
/// `+24`, so the dict's own gOPD arm reads both the value and the
/// per-entry W/E/C flags — which is the whole reason a
/// `defineProperty`'d expando can report `enumerable: false`, a thing
/// no layout field can be.
///
/// # Safety
/// `cell` is a live `Tag::Obj` heap pointer; `key` is a live key cell.
unsafe fn expando_descriptor(cell: *const c_void, key: *const c_void) -> u64 {
    let props = unsafe { crate::obj_own_keys_struct::struct_props(cell) };
    if props.is_null() {
        return VALUE_UNDEFINED_IMM;
    }
    unsafe {
        crate::reflect_get_property_descriptor::__torajs_anyv_get_property_descriptor(
            props as u64,
            key,
        )
    }
}

/// `Object.getOwnPropertyDescriptor` arm for a `Tag::Obj` struct cell.
/// Returns the 4-key descriptor as a NaN-box AnyValue cell, or
/// `undefined` when the key names neither a layout slot nor an
/// expando entry.
///
/// The layout is consulted first and answers every hit, but a miss is
/// not the end of the question: the field list was never the whole
/// set of own properties. `Object.keys` has always walked the expando
/// dict, so conceding `undefined` here left the object disagreeing
/// with itself — the same split the `hasOwnProperty` fold had.
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
        // An anonymous struct interned too late has no layout row at
        // all, but it can still carry expandos.
        return unsafe { expando_descriptor(cell, key) };
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
        let acc = unsafe { struct_accessor_descriptor(cell, layout, key_bytes, key_len, key) };
        if acc != VALUE_UNDEFINED_IMM {
            return acc;
        }
        return unsafe { expando_descriptor(cell, key) };
    }

    // Read the raw 8-byte slot at the field's byte offset, box per type.
    let info = unsafe { __torajs_struct_field_info(layout, idx) };
    let raw = unsafe {
        cell.cast::<u8>()
            .add(info.field_byte_offset as usize)
            .cast::<u64>()
            .read()
    };
    // RFC 20260718-error-message-own-prop — the error-instance
    // `message` own property (§20.5.6.1.1): the own-absence sentinel
    // in the slot means the property does not exist (undefined ctor
    // message / a delete detach). Its `[[Enumerable]]: false` — which
    // it shares with the `stack` header line — is the attribute
    // answer's business, below.
    let hdr_flags = unsafe { cell.cast::<u8>().add(6).cast::<u16>().read() };
    let key_slice = unsafe { core::slice::from_raw_parts(key_bytes, key_len as usize) };
    let is_error = hdr_flags & FLAG_ERROR != 0;
    let is_error_message = is_error && key_slice == b"message";
    // `name` is the mirror case: its attributes are ordinary (an own
    // one only exists because user code assigned it, which is a plain
    // CreateDataProperty), but the sentinel state means no own
    // property at all — §20.5.3.2 leaves it to `Error.prototype`.
    let is_error_absence_slot = is_error_message || (is_error && key_slice == b"name");
    if is_error_absence_slot && raw != 0 && unsafe { __torajs_str_is_undef(raw as *const u8) } != 0
    {
        return VALUE_UNDEFINED_IMM;
    }

    // The descriptor owns its own ref to a heap value — the owned
    // pair decode carries exactly one stake (cell +1 / ShortStr
    // materialization; no separate inc, see field_slot_to_pair_owned).
    let (v_tag, v_val) = unsafe { field_slot_to_pair_owned(info.type_tag, raw) };

    // The three attributes come from the one place that computes them
    // (`declared_member_attrs`): ES §7.3.14 SetIntegrityLevel moves
    // two of them (frozen ⇒ non-writable and non-configurable, sealed
    // ⇒ non-configurable), the Error header lines move the third, and
    // a sidecar left by `Object.defineProperty` outranks all of it.
    // The VALUE is unaffected either way — it never left the layout
    // slot read at the top of this fn.
    let attrs = unsafe { crate::struct_field_attrs::declared_member_attrs(cell, key) };
    use crate::struct_field_attrs::{
        BUCKET_FLAG_CONFIGURABLE, BUCKET_FLAG_ENUMERABLE, BUCKET_FLAG_WRITABLE,
    };
    unsafe {
        crate::reflect::build_data_descriptor(
            v_tag,
            v_val,
            u64::from(attrs & BUCKET_FLAG_WRITABLE != 0),
            u64::from(attrs & BUCKET_FLAG_ENUMERABLE != 0),
            u64::from(attrs & BUCKET_FLAG_CONFIGURABLE != 0),
        )
    }
}

/// `HeapHeader::flags` bit 7 on a `Tag::Obj` cell — [[ErrorData]]
/// (`torajs_rc::FLAG_ERROR` mirror; `error_to_string.rs` twin).
pub(crate) const FLAG_ERROR: u16 = 1 << 7;
