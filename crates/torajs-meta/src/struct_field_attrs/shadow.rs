//! The shadow value a declared field's sidecar may hold (RFC
//! 20260915-declared-field-shadow), as the value-reading faces read
//! it. Moved out of [`super`] whole.

use core::ffi::c_void;

use super::{
    __torajs_anyv_unbox_tag, __torajs_anyv_unbox_value, __torajs_dynobj_iter_key,
    __torajs_dynobj_iter_len, __torajs_dynobj_iter_value, __torajs_is_undef_cell,
    DYNOBJ_HOLE_SENTINEL, OBJ_HDR_FLAG_EXOTIC_FIELD, struct_props,
};

/// The shadow value of the DECLARED field named by `name` (RFC
/// 20260915-declared-field-shadow): a sidecar whose value word is
/// neither the attribute-only filler nor the deletion HOLE holds the
/// value the any lane wrote past the slot's type. Writes the box
/// (BORROWED — the sidecar keeps the stake) to `out` and answers 1;
/// 0 = read the slot. Bytes spelling and the header-bit gate as
/// [`super::__torajs_obj_field_is_deleted`].
///
/// # Safety
/// `cell` is a live `Tag::Obj` heap pointer; `name` points at
/// `name_len` readable bytes; `out` is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_obj_field_shadow(
    cell: *const c_void,
    name: *const u8,
    name_len: u32,
    out: *mut u64,
) -> i64 {
    let flags = unsafe { cell.cast::<u8>().add(6).cast::<u16>().read() };
    if flags & OBJ_HDR_FLAG_EXOTIC_FIELD == 0 {
        return 0;
    }
    let props = unsafe { struct_props(cell) };
    if props.is_null() {
        return 0;
    }
    let want = unsafe { core::slice::from_raw_parts(name, name_len as usize) };
    let n = unsafe { __torajs_dynobj_iter_len(props) };
    for i in 0..n {
        let key = unsafe { __torajs_dynobj_iter_key(props, i) };
        if key.is_null() || unsafe { crate::str_wtf8::StrWtf8::of(key) }.as_bytes() != want {
            continue;
        }
        let w = unsafe { __torajs_dynobj_iter_value(props, i) };
        if w == crate::reflect::VALUE_UNDEFINED_IMM || w == DYNOBJ_HOLE_SENTINEL {
            return 0;
        }
        // A shadowed `undefined` is spelled as the immortal undef
        // cell so it stays distinct from the filler; readers get the
        // immediate back.
        let is_undef_cell = unsafe { __torajs_anyv_unbox_tag(w) } == 4
            && unsafe { __torajs_is_undef_cell(__torajs_anyv_unbox_value(w) as *const u8) } != 0;
        unsafe {
            out.write(if is_undef_cell {
                crate::reflect::VALUE_UNDEFINED_IMM
            } else {
                w
            })
        };
        return 1;
    }
    0
}

/// What a value-reading face decodes for a declared field: the
/// shadow as an Any slot (`type_tag` 0) when one exists, else the
/// slot's own `(type_tag, raw)`. One spelling for the three faces
/// (`Object.values` / gOPD / inspect) so they cannot drift.
///
/// # Safety
/// As [`__torajs_obj_field_shadow`].
pub(crate) unsafe fn slot_or_shadow(
    cell: *const c_void,
    name: *const u8,
    name_len: usize,
    type_tag: u8,
    raw: u64,
) -> (u8, u64) {
    let mut shadow = 0u64;
    if unsafe { __torajs_obj_field_shadow(cell, name, name_len as u32, &mut shadow) } != 0 {
        (0, shadow)
    } else {
        (type_tag, raw)
    }
}
