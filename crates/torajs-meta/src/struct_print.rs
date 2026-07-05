//! W-J Phase D — `console.log(struct_instance)` walker for `Tag::Obj`
//! struct cells. Reads `class_tag@+8` →
//! `__torajs_struct_class_name(class_tag)` for the named-class
//! prefix → `__torajs_struct_field_{count,name,info}` (W-J Phase A4
//! readers over the W-J Phase A3b inner-region rodata) for the
//! per-field walk → boxes each `(tag, val)` slot back into a
//! NaN-box AnyValue with
//! `__torajs_anyv_box_from_pair` and prints via
//! `__torajs_print_anyv_inline` (nested-print substrate trunk).
//!
//! Mirrors `torajs-dynobj/src/print_any.rs::__torajs_obj_print_any`
//! in shape but with the named-class prefix and the static-layout
//! field walk (no insertion-order index, no enumerable filter — every
//! declared field is own / enumerable / configurable by spec for
//! class instances).
//!
//! Output (nested form — caller's `__torajs_print_anyv` Tag::Obj arm
//! appends '\n' at the top-level boundary):
//! - empty:           `Name {}`
//! - non-empty:       `Name {\n  k: v,\n  k2: v2,\n}`
//! - anonymous /      missing layout (class_tag 0 / lookup miss /
//!                    A1 anon-stamp gap): no name prefix —
//!                    `{}` / `{\n  k: v,\n}`.
//!
//! Key emission: declared field names live as raw `__torajs_class_field_name_<i>_<j>`
//! rodata, fetched via `__torajs_struct_field_name`. Unquoted (matches
//! bun's pretty form `Point { x: 1, y: 2 }`).
//!
//! Value emission: each field slot is decoded by
//! `struct_reflect::field_slot_to_pair` into a `(any_slot_tag, value)`
//! pair, re-boxed by `__torajs_anyv_box_from_pair`, and rendered by
//! `__torajs_print_anyv_inline` — the same nested-context renderer
//! `dynobj/print_any.rs` uses, so strings auto-quote, nested structs
//! recurse, primitives format identically to top-level
//! `console.log(<x>)`.

use core::ffi::c_void;

use crate::struct_reflect::field_slot_to_pair;

unsafe extern "C" {
    // torajs-structmeta (W-J Phase A4 + A3c chunk 2) — read-side
    // over __torajs_class_layouts + __torajs_class_name_table.
    fn __torajs_struct_layout_lookup(class_tag: u32) -> *const c_void;
    fn __torajs_struct_field_count(layout: *const c_void) -> u32;
    fn __torajs_struct_field_name(layout: *const c_void, idx: u32) -> StrSlice;
    fn __torajs_struct_field_info(layout: *const c_void, idx: u32) -> FieldInfo;
    fn __torajs_struct_class_name(class_tag: u32) -> StrSlice;

    // torajs-anyvalue::nanbox_encode — re-box (tag, val) into NaN-box
    // AnyValue so the print_anyv_inline nested-context renderer
    // handles every primitive + heap variant identically to top-level
    // console.log.
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;

    // torajs-anyvalue::inspect — indent-threaded nested-context
    // AnyValue printer (inspect indent trunk). Adds `"..."` for
    // strings, recurses into Tag::Arr / Tag::DynObj / Tag::Obj with
    // the field row's indent column.
    fn __torajs_print_anyv_inline_at(v: u64, indent: u32);

    // torajs-io — per-byte stdout writer.
    fn __torajs_io_putc_stdout(c: i32) -> i32;

    // torajs-anyvalue::inspect — line-width estimate primitives
    // (inspect wrap trunk), bun `estimated_line_length` mirror.
    fn __torajs_inspect_line_reset(cols: u32);
    fn __torajs_inspect_line_add(n: u32);
}

/// Mirrors `torajs-structmeta::StrSlice` — `(ptr, len)` borrowed view
/// of a rodata UTF-8 byte slice.
#[repr(C)]
struct StrSlice {
    ptr: *const u8,
    len: usize,
}

/// Mirrors `torajs-structmeta::FieldInfo` — field byte offset within
/// the instance + coarse 8-bit `Type` discriminator.
#[repr(C)]
struct FieldInfo {
    field_byte_offset: u32,
    type_tag: u8,
}

/// Byte offset of the `class_tag` u32 inside a `Tag::Obj` instance
/// (right after the 8-byte universal heap header).
const OBJ_CLASS_TAG_OFF: usize = 8;

#[inline]
unsafe fn put_byte(b: u8) {
    unsafe { __torajs_io_putc_stdout(b as i32) };
}

#[inline]
unsafe fn put_bytes(s: &[u8]) {
    for &b in s {
        unsafe { put_byte(b) };
    }
}

#[inline]
unsafe fn put_bytes_from_raw(ptr: *const u8, len: usize) {
    if ptr.is_null() {
        return;
    }
    for i in 0..len {
        unsafe { put_byte(*ptr.add(i)) };
    }
}

/// W-J Phase D struct walker. Caller (`__torajs_print_anyv` Tag::Obj
/// arm in `crates/torajs-anyvalue/src/inspect/any.rs`) MUST verify
/// `is_cell_imm(v) && heap_type_tag(...) == TAG_OBJ` before invoking;
/// this fn assumes a live Tag::Obj cell.
///
/// Returns nothing — emits the rendered bytes via
/// `__torajs_io_putc_stdout`. No trailing '\n'; the caller appends
/// '\n' at the top-level console.log boundary.
///
/// # Safety
/// `v` must encode a live `Tag::Obj` cell pointer (cell-imm
/// AnyValue encoding). Reads `class_tag@+8` + per-field slot u64s
/// via raw pointer arithmetic against `cell + field_byte_offset`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_struct_print_inline(v: u64) {
    unsafe { __torajs_anyv_struct_print_inline_at(v, 0) }
}

/// Indent-threaded variant (inspect indent trunk) — `indent` is this
/// struct cell's own indent column: fields pad at `indent + 2`, the
/// closing `}` at `indent`, values thread `indent + 2` onward.
///
/// # Safety
/// Same caller contract as [`__torajs_anyv_struct_print_inline`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_struct_print_inline_at(v: u64, indent: u32) {
    let cell = v as *const c_void;
    // SAFETY: caller asserted Tag::Obj cell — class_tag@+8 layout
    // holds by torajs-rc Tag::Obj invariant.
    let class_tag = unsafe {
        cell.cast::<u8>()
            .add(OBJ_CLASS_TAG_OFF)
            .cast::<u32>()
            .read()
    };
    let layout = unsafe { __torajs_struct_layout_lookup(class_tag) };
    let n = unsafe { __torajs_struct_field_count(layout) };

    // Named-class prefix. NULL / empty class_name → no prefix
    // (anonymous A1 anon-stamp coverage gap → degrade to bare
    // `{…}` form so the walker still shows useful content).
    let cn = unsafe { __torajs_struct_class_name(class_tag) };
    if !cn.ptr.is_null() && cn.len > 0 {
        unsafe { put_bytes_from_raw(cn.ptr, cn.len) };
        unsafe { put_byte(b' ') };
    }

    if n == 0 {
        unsafe { put_bytes(b"{}") };
        return;
    }

    unsafe { put_bytes(b"{\n") };
    // bun handleFirstProperty estimate seed — see
    // torajs-dynobj::print_any for the accumulation rationale.
    unsafe { __torajs_inspect_line_reset(indent + 1) };
    let mut i: u32 = 0;
    while i < n {
        if i > 0 {
            unsafe { put_bytes(b",\n") };
            unsafe { __torajs_inspect_line_add(1) };
        }
        unsafe { put_indent(indent + 2) };
        let name = unsafe { __torajs_struct_field_name(layout, i) };
        let info = unsafe { __torajs_struct_field_info(layout, i) };
        // SAFETY: field_byte_offset is the SSA-emitted slot offset;
        // every well-formed Tag::Obj cell has `n` slots laid out
        // starting at OBJ_HEADER_SIZE (8) + i*8 by struct construction.
        let raw = unsafe {
            cell.cast::<u8>()
                .add(info.field_byte_offset as usize)
                .cast::<u64>()
                .read()
        };
        unsafe { put_bytes_from_raw(name.ptr, name.len) };
        unsafe { put_bytes(b": ") };
        unsafe { __torajs_inspect_line_add(name.len as u32 + 2) };
        let (tag, val) = unsafe { field_slot_to_pair(info.type_tag, raw) };
        let anyv = unsafe { __torajs_anyv_box_from_pair(tag as i64, val as i64) };
        unsafe { __torajs_print_anyv_inline_at(anyv, indent + 2) };
        i += 1;
    }
    unsafe { put_bytes(b",\n") };
    unsafe { __torajs_inspect_line_add(1) };
    unsafe { put_indent(indent) };
    unsafe { put_byte(b'}') };
}

#[inline]
unsafe fn put_indent(n: u32) {
    for _ in 0..n {
        unsafe { put_byte(b' ') };
    }
}

/// Top-level entry — emits the nested-form walker output plus a
/// trailing '\n' so the SSA dispatcher's `console.log(<struct>)`
/// path can wire it identically to other typed-receiver printers
/// (`__torajs_map_print` / `__torajs_obj_print_any` outer wrapper
/// pattern).
///
/// Caller MUST verify Tag::Obj before invoking.
///
/// # Safety
/// Same caller contract as [`__torajs_anyv_struct_print_inline`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_struct_print(v: u64) {
    // Fresh console.log line — reset the wrap estimate to column 0.
    unsafe { __torajs_inspect_line_reset(0) };
    unsafe { __torajs_anyv_struct_print_inline(v) };
    unsafe { put_byte(b'\n') };
}
