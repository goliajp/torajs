//! The DATA-FIELD half of the struct probe — split from the parent
//! when the accessor half's twin-primary lane pushed the file at the
//! 500-line limit. The parent answers "how is an accessor resolved
//! and invoked"; this answers "what does a layout data slot hold".
//! Bodies verbatim; a child module reaches the parent's private
//! externs, constants and `FieldInfo` mirror directly.

use torajs_rc::AnySlotTag;

use super::*;

/// `Tag::Obj` struct-cell field probe — the class-layout reflection
/// walk (`struct_reflect::struct_cell_descriptor` twin): class_tag
/// (u32 @ +8) → layout entry → field_find by key bytes → raw 8-byte
/// slot decoded per the field's coarse type_tag. Answers the
/// `(any_tag, payload)` pair on a hit — borrow-shaped like the
/// dynobj probe (the struct keeps its stake) — or `None` for a
/// missing layout / absent field. Chunk 744: pre-fix a struct cell
/// fell to the builtin-reify arm and every field read through an
/// `any` receiver whose sid the compile-time IC couldn't see (a
/// Pass 2 fresh literal in a later-lowered fn) answered a silent
/// `undefined`.
///
/// # Safety
/// `ptr` is a live `Tag::Obj` heap pointer; `key` is a live Str cell.
pub(crate) unsafe fn struct_field_pair(ptr: *mut c_void, key: *const c_void) -> Option<(u64, u64)> {
    let k = unsafe { torajs_rc::str_wtf8::StrWtf8::of(key) };
    unsafe { struct_field_pair_bytes(ptr, k.as_bytes()) }
}

/// [`struct_field_pair`] keyed by raw name bytes — the shape the
/// iteration kernel needs, which probes fixed field names (`done` /
/// `value`) and has no Str cell to spend an allocation on per step.
///
/// # Safety
/// `ptr` is a live `Tag::Obj` heap pointer.
pub(crate) unsafe fn struct_field_pair_bytes(ptr: *mut c_void, name: &[u8]) -> Option<(u64, u64)> {
    let (type_tag, raw, slot) = unsafe { struct_field_raw(ptr, name) }?;
    Some(match type_tag {
        // Any-typed field: the slot is a NaN-box — decode it. A
        // ShortStr box first normalizes IN the slot to its
        // materialized heap Str (546-02 M1 family): this pair is
        // borrow-shaped — the struct keeps the stake — and a
        // materialization minted per probe has no owner (one leaked
        // Str per `any`-receiver read of a short-string field, twice
        // per member get since tag and value channel each probe).
        // After the write-back the slot owns the rc=1 cell and every
        // later read rides the plain Heap arm; the two spellings are
        // the same string value to every Any-slot consumer.
        0 => {
            if crate::nanbox::is_short_str(raw) {
                let mat = crate::nanbox_encode::__torajs_anyv_unbox_value(raw);
                let boxed = crate::nanbox::box_void_ptr(mat as *mut c_void);
                unsafe { slot.write(boxed) };
                return Some((AnySlotTag::Heap as u64, mat as u64));
            }
            (
                crate::nanbox_encode::__torajs_anyv_unbox_tag(raw) as u64,
                crate::nanbox_encode::__torajs_anyv_unbox_value(raw) as u64,
            )
        }
        1 => (AnySlotTag::I64 as u64, raw),
        // A `number` slot holding the F64 undefined sentinel (a
        // generator step's completion value, rotation 640) is
        // `undefined` to the any world, as the heap arm's cells are.
        2 if raw == torajs_rc::F64_UNDEF_SENTINEL_BITS => (AnySlotTag::Undef as u64, 0),
        2 => (AnySlotTag::F64 as u64, raw),
        3 => (AnySlotTag::Bool as u64, raw),
        // Heap slot — undefined sentinels normalize to JS
        // `undefined` (RFC 20260710 C1/C2b; the meta twin
        // `field_slot_to_anyv_borrowed` already does), so a
        // detached error `message` never leaks the 9-char cell.
        _ => {
            if raw != 0
                && (unsafe { __torajs_str_is_undef(raw as *const u8) } != 0
                    || unsafe { __torajs_is_undef_cell(raw as *const u8) } != 0)
            {
                (AnySlotTag::Undef as u64, 0)
            } else {
                (AnySlotTag::Heap as u64, raw)
            }
        }
    })
}

unsafe extern "C" {
    /// torajs-str — undefined sentinel identity probe (RFC 20260710
    /// C1).
    fn __torajs_str_is_undef(p: *const u8) -> i64;
    /// torajs-rc — generic undefined oddball probe (RFC 20260710
    /// C2b).
    fn __torajs_is_undef_cell(p: *const u8) -> i64;
    /// torajs-meta — is this declared field deleted (RFC 20260915)?
    fn __torajs_obj_field_is_deleted(cell: *const c_void, name: *const u8, name_len: u32) -> i64;
    /// torajs-meta — the field's shadow value, when the any lane wrote
    /// one past the slot's type (RFC 20260915-declared-field-shadow).
    fn __torajs_obj_field_shadow(
        cell: *const c_void,
        name: *const u8,
        name_len: u32,
        out: *mut u64,
    ) -> i64;
    /// torajs-dynobj — drops the deletion record on revive; over a
    /// deleted accessor it stores a fresh data entry instead and
    /// answers 1 (`(tag, value)` transferred).
    fn __torajs_obj_declared_field_revive(
        obj: *mut c_void,
        key: *mut c_void,
        tag: u64,
        value: u64,
    ) -> i64;
    fn __torajs_dynobj_has(obj: *const c_void, key: *const c_void) -> i32;
    fn __torajs_dynobj_get_tag(obj: *const c_void, key: *const c_void) -> u64;
}

/// RFC 20260915 — is the DECLARED field `key` of `ptr` deleted?
///
/// # Safety
/// `ptr` is a live `Tag::Obj` heap pointer; `key` a live Str cell.
pub(crate) unsafe fn declared_field_deleted(ptr: *const c_void, key: *const c_void) -> bool {
    let k = unsafe { torajs_rc::str_wtf8::StrWtf8::of(key) };
    unsafe { __torajs_obj_field_is_deleted(ptr, k.as_ptr(), k.len()) != 0 }
}

/// Does the layout of `ptr` declare `key` (data field or accessor)?
/// Deletion-blind on purpose: this is the sidecar invariant's question
/// ("a declared name in the `+24` dict can only be a sidecar"), not
/// the own-property one — so an accessor an ordinary property replaced
/// answers no: its dict entry is that property.
///
/// # Safety
/// `ptr` is a live `Tag::Obj` heap pointer; `key` a live Str cell.
pub(crate) unsafe fn struct_layout_declares(ptr: *const c_void, key: *const c_void) -> bool {
    let class_tag = unsafe { ptr.cast::<u8>().add(OBJ_CLASS_TAG_OFF).cast::<u32>().read() };
    let layout = unsafe { __torajs_struct_layout_lookup(class_tag) };
    if layout.is_null() {
        return false;
    }
    let k = unsafe { torajs_rc::str_wtf8::StrWtf8::of(key) };
    let (b, n) = (k.as_ptr(), k.len());
    unsafe {
        __torajs_struct_field_find(layout, b, n) != u32::MAX
            || ((__torajs_struct_accessor_find(layout, b, n, 0) != u32::MAX
                || __torajs_struct_accessor_find(layout, b, n, 1) != u32::MAX)
                && super::accessor_state(ptr, k.as_bytes()) != super::AccessorState::Replaced)
    }
}

/// The struct receiver's expando own-face probe: an entry under `key`
/// that the layout does NOT declare. A declared name in the dict is an
/// attribute sidecar (RFC 20260806) or a deletion record (RFC
/// 20260915), never an own expando — the one place that invariant is
/// spelled for `has` / `get` / the existence probe.
///
/// # Safety
/// `ptr` is a live `Tag::Obj` heap pointer; `key` a live Str cell.
/// Whether `v` boxes an `AccessorPair` cell.
///
/// # Safety
/// A heap payload of `v` is a live cell.
unsafe fn is_accessor_pair_box(v: u64) -> bool {
    if crate::nanbox_encode::__torajs_anyv_unbox_tag(v) as u64 != AnySlotTag::Heap as u64 {
        return false;
    }
    let cell = crate::nanbox_encode::__torajs_anyv_unbox_value(v) as *const u8;
    !cell.is_null()
        && unsafe { cell.add(4).cast::<u16>().read() } == torajs_rc::Tag::AccessorPair as u16
}

/// Whether `key` names a DECLARED field that was redefined as an
/// accessor — its AccessorPair lives in the `+24` sidecar entry,
/// which is an own property with the ordinary dict accessor faces.
///
/// # Safety
/// `ptr` is a live `Tag::Obj` heap pointer; `key` a live Str cell.
pub(crate) unsafe fn declared_accessor_sidecar(ptr: *const c_void, key: *const c_void) -> bool {
    let props = unsafe { crate::member_get_layout::struct_props(ptr) };
    !props.is_null()
        && unsafe { __torajs_dynobj_has(props, key) } != 0
        && unsafe { __torajs_dynobj_get_tag(props, key) } == crate::struct_probe::ANY_ACCESSOR_TAG
        && unsafe { struct_layout_declares(ptr, key) }
}

pub(crate) unsafe fn expando_own_has(ptr: *const c_void, key: *const c_void) -> bool {
    let props = unsafe { crate::member_get_layout::struct_props(ptr) };
    !props.is_null()
        && unsafe { __torajs_dynobj_has(props, key) } != 0
        && !unsafe { struct_layout_declares(ptr, key) }
}

/// [[Set]] on a deleted declared field is CreateDataProperty (RFC
/// 20260915): refused on a non-extensible instance (the refusal
/// consumes the payload per `throw_on_refusal`), otherwise the
/// deletion record goes and the caller's ordinary declared-field
/// store follows — default attributes, no sidecar. A deleted accessor
/// takes the store as a new data property instead (`Some(1)`).
/// `Some(0)` = refused; `None` = not deleted, or revived: go on.
///
/// # Safety
/// `ptr` is a live `Tag::Obj` heap pointer; `key` a live Str cell;
/// `(tag, value)` is the caller's owned payload.
pub(crate) unsafe fn deleted_field_write_gate(
    ptr: *mut c_void,
    key: *const c_void,
    tag: u64,
    value: u64,
    throw_on_refusal: bool,
) -> Option<i64> {
    if !unsafe { declared_field_deleted(ptr, key) } {
        return None;
    }
    if unsafe {
        crate::member_set_closure::refuse_new_key_non_extensible(
            ptr,
            false,
            tag,
            value,
            throw_on_refusal,
        )
    } {
        return Some(0);
    }
    // a deleted accessor comes back as a data property, stored there
    if unsafe { __torajs_obj_declared_field_revive(ptr, key.cast_mut(), tag, value) } != 0 {
        return Some(1);
    }
    None
}

/// Layout-resolved raw slot read shared by the pair / anyv decode
/// shapes: `(layout type_tag, raw 8-byte slot bits, slot address)`
/// for a hit, `None` for a missing layout / absent field /
/// accessor-slot spelling. The slot address feeds the pair shape's
/// in-slot ShortStr normalization; the anyv shape ignores it.
///
/// # Safety
/// `ptr` is a live `Tag::Obj` heap pointer.
unsafe fn struct_field_raw(ptr: *mut c_void, name: &[u8]) -> Option<(u8, u64, *mut u64)> {
    // RFC 20260714-objlit-accessor blade 5 — the accessor SLOT spelling
    // is not a property. `find_field` resolves `__getter_v` (the layout
    // really does carry that field), so without this guard an `any` read
    // of `o.__getter_v` handed the getter closure back as a value — the
    // internal name leaking onto the user-visible surface. bun: absent.
    if unsafe { __torajs_accessor_name_kind(name.as_ptr(), name.len() as u32) } != 255 {
        return None;
    }
    let class_tag = unsafe { ptr.cast::<u8>().add(OBJ_CLASS_TAG_OFF).cast::<u32>().read() };
    let layout = unsafe { __torajs_struct_layout_lookup(class_tag) };
    if layout.is_null() {
        return None;
    }
    let idx = unsafe { __torajs_struct_field_find(layout, name.as_ptr(), name.len() as u32) };
    if idx == u32::MAX {
        return None;
    }
    // RFC 20260915 — a deleted declared field is not an own property;
    // the slot's bits are stale.
    if unsafe { __torajs_obj_field_is_deleted(ptr, name.as_ptr(), name.len() as u32) } != 0 {
        return None;
    }
    // RFC 20260915-declared-field-shadow — a value the slot could not
    // hold reads as an Any slot from the sidecar. The store spelled a
    // ShortStr as its materialized cell, so the pair shape's in-slot
    // normalization never fires on this null slot address.
    let mut shadow = 0u64;
    if unsafe { __torajs_obj_field_shadow(ptr, name.as_ptr(), name.len() as u32, &mut shadow) } != 0
    {
        // A field redefined as an accessor (`Object.defineProperty(
        // inst, "x", { get })`) parks its AccessorPair here: it is no
        // longer a DATA field, and the readers find it in the dict
        // with the accessor faces every expando entry already has
        // ([`declared_accessor_sidecar`]). Reading it as data answered
        // the pair itself (`[object]`).
        if unsafe { is_accessor_pair_box(shadow) } {
            return None;
        }
        return Some((0, shadow, core::ptr::null_mut()));
    }
    let info = unsafe { __torajs_struct_field_info(layout, idx) };
    let slot = unsafe {
        ptr.cast::<u8>()
            .add(info.field_byte_offset as usize)
            .cast::<u64>()
    };
    let raw = unsafe { slot.read() };
    Some((info.type_tag, raw, slot))
}

/// C ABI shell over [`struct_field_raw`] for sibling runtime crates
/// (torajs-dynobj's §6.2.6.5 ToPropertyDescriptor struct-desc lane).
/// Answers 1 and fills `out_anyv` with the field as a BORROWED
/// NaN-box (an Any slot passes its box through untouched — no
/// ShortStr materialization; typed slots pure-encode, the struct
/// keeps every heap stake), or 0 for missing layout / absent field.
///
/// # Safety
/// `obj` is a live `Tag::Obj` heap pointer; `name` points at
/// `name_len` readable bytes; `out_anyv` is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_struct_field_read_anyv(
    obj: *mut c_void,
    name: *const u8,
    name_len: u32,
    out_anyv: *mut u64,
) -> i64 {
    let bytes = unsafe { core::slice::from_raw_parts(name, name_len as usize) };
    let Some((type_tag, raw, _)) = (unsafe { struct_field_raw(obj, bytes) }) else {
        return 0;
    };
    let anyv = if type_tag == 0 {
        raw
    } else if type_tag == 2 && raw == torajs_rc::F64_UNDEF_SENTINEL_BITS {
        // Same normalization as `struct_field_pair_bytes`' F64 arm.
        unsafe { crate::nanbox_encode::__torajs_anyv_box_from_pair(AnySlotTag::Undef as i64, 0) }
    } else if type_tag >= 4 && raw == 0 {
        // a heap slot's NULL is JS null, its undefined cells undefined
        unsafe { crate::nanbox_encode::__torajs_anyv_box_from_pair(AnySlotTag::Null as i64, 0) }
    } else if type_tag >= 4
        && (unsafe { __torajs_str_is_undef(raw as *const u8) } != 0
            || unsafe { __torajs_is_undef_cell(raw as *const u8) } != 0)
    {
        unsafe { crate::nanbox_encode::__torajs_anyv_box_from_pair(AnySlotTag::Undef as i64, 0) }
    } else {
        let slot_tag = match type_tag {
            1 => AnySlotTag::I64,
            2 => AnySlotTag::F64,
            3 => AnySlotTag::Bool,
            _ => AnySlotTag::Heap,
        };
        unsafe { crate::nanbox_encode::__torajs_anyv_box_from_pair(slot_tag as i64, raw as i64) }
    };
    unsafe { *out_anyv = anyv };
    1
}
