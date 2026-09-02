//! W-J Phase D — `console.log(struct_instance)` walker for `Tag::Obj`
//! struct cells. Reads `class_tag@+8` →
//! `__torajs_struct_class_name(class_tag)` for the named-class
//! prefix → `__torajs_struct_field_{count,name,info}` (W-J Phase A4
//! readers over the W-J Phase A3b inner-region rodata) for the
//! per-field walk → decodes each slot to a borrowed NaN-box
//! AnyValue and prints via
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
//! `struct_reflect::field_slot_to_anyv_borrowed` into a borrowed
//! NaN-box AnyValue (zero rc traffic — an Any slot's box, ShortStr
//! included, passes through verbatim) and rendered by
//! `__torajs_print_anyv_inline` — the same nested-context renderer
//! `dynobj/print_any.rs` uses, so strings auto-quote, nested structs
//! recurse, primitives format identically to top-level
//! `console.log(<x>)`.

//! The row emission itself — the layout fields, the expando
//! entries and the prototype methods — lives in
//! [`crate::struct_print_rows`]; this file holds the name prefix,
//! the shared byte writers and the FFI seams.

use core::ffi::c_void;

unsafe extern "C" {
    // torajs-structmeta (W-J Phase A3c chunk 2) — the class name
    // table read side, the prefix this file writes.
    fn __torajs_struct_class_name(class_tag: u32) -> StrSlice;

    // torajs-io — per-byte stdout writer.
    pub(crate) fn __torajs_io_putc_out(c: i32) -> i32;

    // torajs-anyvalue::inspect — line-width estimate primitive
    // (inspect wrap trunk), bun `estimated_line_length` mirror.
    pub(crate) fn __torajs_inspect_line_reset(cols: u32);
}

/// Mirrors `torajs-structmeta::StrSlice` — `(ptr, len)` borrowed view
/// of a rodata UTF-8 byte slice.
#[repr(C)]
pub(crate) struct StrSlice {
    pub(crate) ptr: *const u8,
    pub(crate) len: usize,
}

/// Byte offset of the `class_tag` u32 inside a `Tag::Obj` instance
/// (right after the 8-byte universal heap header).
pub(crate) const OBJ_CLASS_TAG_OFF: usize = 8;

#[inline]
pub(crate) unsafe fn put_byte(b: u8) {
    unsafe { __torajs_io_putc_out(b as i32) };
}

#[inline]
pub(crate) unsafe fn put_bytes(s: &[u8]) {
    for &b in s {
        unsafe { put_byte(b) };
    }
}

#[inline]
pub(crate) unsafe fn put_bytes_from_raw(ptr: *const u8, len: usize) {
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
/// `__torajs_io_putc_out`. No trailing '\n'; the caller appends
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

    // Named-class prefix. NULL / empty class_name → no prefix
    // (anonymous A1 anon-stamp coverage gap → degrade to bare
    // `{…}` form so the walker still shows useful content).
    let cn = unsafe { __torajs_struct_class_name(class_tag) };
    if !cn.ptr.is_null() && cn.len > 0 {
        unsafe { put_bytes_from_raw(cn.ptr, cn.len) };
        unsafe { put_byte(b' ') };
    }
    unsafe { crate::struct_print_rows::put_body(cell, class_tag, indent) };
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
