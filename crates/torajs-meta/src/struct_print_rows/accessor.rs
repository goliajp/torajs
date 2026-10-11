//! bun's rendering of an accessor row: one row per get/set pair, and
//! which of `[Getter]` / `[Setter]` / `[Getter/Setter]` it shows.
//! Moved out of [`super`] whole.

use core::ffi::c_void;

use super::{__torajs_struct_field_count, __torajs_struct_field_name};
use crate::struct_reflect::{ACC_GETTER, ACC_SETTER, accessor_slot_name, slot_key};

/// Whether an accessor slot for `key` was already rendered by an
/// earlier slot — the two halves of a get/set pair are one entry.
///
/// # Safety
/// `layout` is a live layout entry; `key` points at readable bytes.
pub(super) unsafe fn accessor_emitted_before(
    layout: *const c_void,
    upto: u32,
    key: (*const u8, usize),
) -> bool {
    let mut j = 0;
    while j < upto {
        let prev = unsafe { __torajs_struct_field_name(layout, j) };
        if unsafe { accessor_slot_name(prev.ptr, prev.len) }.is_none() {
            j += 1;
            continue;
        }
        let (p, plen) = unsafe { slot_key(prev.ptr, prev.len) };
        if plen == key.1
            && unsafe {
                core::slice::from_raw_parts(p, plen) == core::slice::from_raw_parts(key.0, key.1)
            }
        {
            return true;
        }
        j += 1;
    }
    false
}

/// bun's accessor rendering for the property `key` of this layout.
///
/// # Safety
/// `layout` is a live layout entry; `key` points at `key_len` readable
/// bytes.
pub(super) unsafe fn accessor_tag(
    layout: *const c_void,
    key: *const u8,
    key_len: usize,
) -> &'static [u8] {
    let n = unsafe { __torajs_struct_field_count(layout) };
    let (mut has_get, mut has_set) = (false, false);
    let mut i = 0;
    while i < n {
        let name = unsafe { __torajs_struct_field_name(layout, i) };
        if let Some((kind, p, plen)) = unsafe { accessor_slot_name(name.ptr, name.len) }
            && plen == key_len
            && unsafe {
                core::slice::from_raw_parts(p, plen) == core::slice::from_raw_parts(key, key_len)
            }
        {
            has_get |= kind == ACC_GETTER;
            has_set |= kind == ACC_SETTER;
        }
        i += 1;
    }
    match (has_get, has_set) {
        (true, true) => b"[Getter/Setter]",
        (false, true) => b"[Setter]",
        _ => b"[Getter]",
    }
}
