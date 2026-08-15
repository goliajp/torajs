//! 405-05 — the prototype-method entries of the struct inspect
//! walker. bun's default inspect lists a class instance's prototype
//! methods after its own properties (`NG { k: 6, get: [Function:
//! get] }`) and renders prototype accessors as `[Getter]` /
//! `[Setter]` / `[Getter/Setter]`; tr's `struct_print` walked only
//! `field_metadata`, so every method-carrying class printed bare.
//!
//! Reads the same `.__class_methods_<i>` table the reify walk does
//! (`classmeta/reify.rs`): `__ccm_` computed-name sentinels are
//! skipped, `__getter_<p>` / `__setter_<p>` synthetic names collapse
//! into one accessor entry per property key. The table merges parent
//! chains, so inherited methods list too — the same order bun shows
//! for subclass instances (own first) holds only if the table is
//! own-first; verified against bun by fixture.

use core::ffi::c_void;

use crate::struct_reflect::{ACC_GETTER, ACC_SETTER, accessor_slot_name, slot_key};

unsafe extern "C" {
    fn __torajs_struct_layout_lookup(class_tag: u32) -> *const c_void;
    fn __torajs_struct_method_count(layout: *const c_void) -> u32;
    fn __torajs_struct_method_at(
        layout: *const c_void,
        idx: u32,
        name_ptr: *mut *const u8,
        name_len: *mut u32,
    ) -> *const c_void;
    fn __torajs_io_putc_out(c: i32) -> i32;
    fn __torajs_inspect_line_add(n: u32);
}

#[inline]
unsafe fn put_bytes(s: &[u8]) {
    for &b in s {
        unsafe { __torajs_io_putc_out(b as i32) };
    }
}

/// The `(name, adapter)` of row `i`, or `None` for a row the inspect
/// face skips (null adapter / empty name / `__ccm_` computed-name
/// sentinel — the reify walk's exact filter).
unsafe fn visible_row(layout: *const c_void, i: u32) -> Option<(*const u8, usize)> {
    let mut name_ptr: *const u8 = core::ptr::null();
    let mut name_len: u32 = 0;
    let adapter = unsafe { __torajs_struct_method_at(layout, i, &mut name_ptr, &mut name_len) };
    if adapter.is_null() || name_ptr.is_null() || name_len == 0 {
        return None;
    }
    let name = unsafe { core::slice::from_raw_parts(name_ptr, name_len as usize) };
    if name.starts_with(b"__ccm_") {
        return None;
    }
    Some((name_ptr, name_len as usize))
}

/// Whether an accessor row for `key` appears in rows `[0, upto)` —
/// the two halves of a get/set pair are one printed entry.
unsafe fn accessor_row_before(layout: *const c_void, upto: u32, key: (*const u8, usize)) -> bool {
    for j in 0..upto {
        let Some((np, nl)) = (unsafe { visible_row(layout, j) }) else {
            continue;
        };
        if unsafe { accessor_slot_name(np, nl) }.is_none() {
            continue;
        }
        let (p, plen) = unsafe { slot_key(np, nl) };
        if plen == key.1
            && unsafe {
                core::slice::from_raw_parts(p, plen) == core::slice::from_raw_parts(key.0, key.1)
            }
        {
            return true;
        }
    }
    false
}

/// bun's accessor rendering for the prototype property `key`.
unsafe fn accessor_row_tag(layout: *const c_void, key: *const u8, key_len: usize) -> &'static [u8] {
    let n = unsafe { __torajs_struct_method_count(layout) };
    let (mut has_get, mut has_set) = (false, false);
    for i in 0..n {
        let Some((np, nl)) = (unsafe { visible_row(layout, i) }) else {
            continue;
        };
        if let Some((kind, p, plen)) = unsafe { accessor_slot_name(np, nl) }
            && plen == key_len
            && unsafe {
                core::slice::from_raw_parts(p, plen) == core::slice::from_raw_parts(key, key_len)
            }
        {
            has_get |= kind == ACC_GETTER;
            has_set |= kind == ACC_SETTER;
        }
    }
    match (has_get, has_set) {
        (true, true) => b"[Getter/Setter]",
        (false, true) => b"[Setter]",
        _ => b"[Getter]",
    }
}

/// True when the class has at least one printable prototype entry —
/// feeds the caller's empty-form (`Name {}`) early return.
pub(crate) unsafe fn has_visible_methods(class_tag: u32) -> bool {
    let layout = unsafe { __torajs_struct_layout_lookup(class_tag) };
    if layout.is_null() {
        return false;
    }
    let n = unsafe { __torajs_struct_method_count(layout) };
    (0..n).any(|i| unsafe { visible_row(layout, i) }.is_some())
}

/// Emit the prototype-method entries at `indent + 2`, continuing the
/// caller's separator protocol (`emitted > 0` → leading `,\n`).
pub(crate) unsafe fn print_proto_methods(class_tag: u32, indent: u32, emitted: &mut u32) {
    let layout = unsafe { __torajs_struct_layout_lookup(class_tag) };
    if layout.is_null() {
        return;
    }
    let n = unsafe { __torajs_struct_method_count(layout) };
    for i in 0..n {
        let Some((np, nl)) = (unsafe { visible_row(layout, i) }) else {
            continue;
        };
        let kind = unsafe { accessor_slot_name(np, nl) };
        let (kp, klen) = unsafe { slot_key(np, nl) };
        if kind.is_some() && unsafe { accessor_row_before(layout, i, (kp, klen)) } {
            continue;
        }
        if *emitted > 0 {
            unsafe { put_bytes(b",\n") };
            unsafe { __torajs_inspect_line_add(1) };
        }
        for _ in 0..indent + 2 {
            unsafe { __torajs_io_putc_out(b' ' as i32) };
        }
        unsafe { put_bytes(core::slice::from_raw_parts(kp, klen)) };
        unsafe { put_bytes(b": ") };
        unsafe { __torajs_inspect_line_add(klen as u32 + 2) };
        if kind.is_some() {
            let tag = unsafe { accessor_row_tag(layout, kp, klen) };
            unsafe { put_bytes(tag) };
            unsafe { __torajs_inspect_line_add(tag.len() as u32) };
        } else {
            // bun's method rendering: `[Function: <name>]`.
            unsafe { put_bytes(b"[Function: ") };
            unsafe { put_bytes(core::slice::from_raw_parts(kp, klen)) };
            unsafe { put_bytes(b"]") };
            unsafe { __torajs_inspect_line_add(klen as u32 + 12) };
        }
        *emitted += 1;
    }
}
