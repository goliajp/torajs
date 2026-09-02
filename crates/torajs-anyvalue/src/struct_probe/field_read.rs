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
