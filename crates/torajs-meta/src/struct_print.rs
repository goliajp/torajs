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

use core::ffi::c_void;

use crate::struct_reflect::{
    ACC_GETTER, ACC_SETTER, accessor_slot_name, field_slot_to_anyv_borrowed, slot_key,
};

/// Whether an accessor slot for `key` was already rendered by an
/// earlier slot — the two halves of a get/set pair are one entry.
///
/// # Safety
/// `layout` is a live layout entry; `key` points at readable bytes.
unsafe fn accessor_emitted_before(
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
unsafe fn accessor_tag(layout: *const c_void, key: *const u8, key_len: usize) -> &'static [u8] {
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

unsafe extern "C" {
    // torajs-structmeta (W-J Phase A4 + A3c chunk 2) — read-side
    // over __torajs_class_layouts + __torajs_class_name_table.
    fn __torajs_struct_layout_lookup(class_tag: u32) -> *const c_void;
    fn __torajs_struct_field_count(layout: *const c_void) -> u32;
    fn __torajs_struct_field_name(layout: *const c_void, idx: u32) -> StrSlice;
    fn __torajs_struct_field_info(layout: *const c_void, idx: u32) -> FieldInfo;
    fn __torajs_struct_class_name(class_tag: u32) -> StrSlice;

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

    // torajs-dynobj — expando-dict walk (blade 3; the own-keys
    // walker's iter trio) + entry pair read.
    fn __torajs_dynobj_iter_len(obj: *const c_void) -> u64;
    fn __torajs_dynobj_iter_key(obj: *const c_void, i: u64) -> *mut c_void;
    fn __torajs_dynobj_iter_order(obj: *const c_void, out: *mut u64, cap: u64) -> u64;
    fn __torajs_dynobj_iter_flags(obj: *const c_void, i: u64) -> u64;
    fn __torajs_dynobj_get_tag(obj: *const c_void, key: *const u8) -> u64;
    fn __torajs_dynobj_get_value(obj: *const c_void, key: *const u8) -> u64;
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
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

    // Blade 3 (RFC 20260714-struct-dynamic-props) — expando entries
    // render after the layout fields (insertion order; enumerable
    // only, matching the keys face).
    let props = unsafe { crate::obj_own_keys_struct::struct_props(cell) };
    let n_props = if props.is_null() {
        0
    } else {
        unsafe { __torajs_dynobj_iter_len(props) }
    };

    if n == 0 && n_props == 0 {
        unsafe { put_bytes(b"{}") };
        return;
    }

    unsafe { put_bytes(b"{\n") };
    // bun handleFirstProperty estimate seed — see
    // torajs-dynobj::print_any for the accumulation rationale.
    unsafe { __torajs_inspect_line_reset(indent + 1) };
    let mut i: u32 = 0;
    // Accessor slots collapse a get/set pair into one entry, so the
    // separator keys off what was actually emitted, not off `i`.
    let mut emitted: u32 = 0;
    while i < n {
        let name = unsafe { __torajs_struct_field_name(layout, i) };
        let kind = unsafe { accessor_slot_name(name.ptr, name.len) };
        let (kp, klen) = unsafe { slot_key(name.ptr, name.len) };
        // The second half of a pair was already rendered under the
        // same key by the first (RFC 20260714-objlit-accessor).
        if kind.is_some() && unsafe { accessor_emitted_before(layout, i, (kp, klen)) } {
            i += 1;
            continue;
        }
        if emitted > 0 {
            unsafe { put_bytes(b",\n") };
            unsafe { __torajs_inspect_line_add(1) };
        }
        unsafe { put_indent(indent + 2) };
        unsafe { put_bytes_from_raw(kp, klen) };
        unsafe { put_bytes(b": ") };
        unsafe { __torajs_inspect_line_add(klen as u32 + 2) };
        if kind.is_some() {
            // bun renders the accessor itself, never the closure:
            // `[Getter]` / `[Setter]` / `[Getter/Setter]`.
            let tag = unsafe { accessor_tag(layout, kp, klen) };
            unsafe { put_bytes(tag) };
            unsafe { __torajs_inspect_line_add(tag.len() as u32) };
            emitted += 1;
            i += 1;
            continue;
        }
        let info = unsafe { __torajs_struct_field_info(layout, i) };
        // SAFETY: field_byte_offset is the SSA-emitted slot offset;
        // every well-formed Tag::Obj cell has `n` slots laid out
        // starting at OBJ_HEADER_SIZE (32) + i*8 by struct construction.
        let raw = unsafe {
            cell.cast::<u8>()
                .add(info.field_byte_offset as usize)
                .cast::<u64>()
                .read()
        };
        // Borrowed anyv decode — an Any slot's box (ShortStr
        // included) prints verbatim with zero rc traffic; the
        // pre-fix pair roundtrip materialized a ShortStr slot into
        // an owned Str this print walk never dropped.
        let anyv = unsafe { field_slot_to_anyv_borrowed(info.type_tag, raw) };
        unsafe { __torajs_print_anyv_inline_at(anyv, indent + 2) };
        emitted += 1;
        i += 1;
    }
    // Blade 3 — expando entries (insertion order, enumerable only;
    // the pair read is a borrow, boxed for the inline printer with
    // zero rc traffic).
    if n_props > 0 {
        let mut order = vec![0u64; n_props as usize];
        let n_ord = unsafe { __torajs_dynobj_iter_order(props, order.as_mut_ptr(), n_props) };
        for &pi in order.iter().take(n_ord as usize) {
            let flags = unsafe { __torajs_dynobj_iter_flags(props, pi) };
            if flags & crate::obj_own_keys::FLAG_ENUMERABLE == 0 {
                continue;
            }
            let key = unsafe { __torajs_dynobj_iter_key(props, pi) };
            if key.is_null() {
                continue;
            }
            if emitted > 0 {
                unsafe { put_bytes(b",\n") };
                unsafe { __torajs_inspect_line_add(1) };
            }
            let klen = unsafe { key.cast::<u8>().add(8).cast::<u32>().read() } as usize;
            let kbytes = unsafe { key.cast::<u8>().add(16) };
            unsafe { put_indent(indent + 2) };
            unsafe { put_bytes_from_raw(kbytes, klen) };
            unsafe { put_bytes(b": ") };
            unsafe { __torajs_inspect_line_add(klen as u32 + 2) };
            // The kernel keys by the live Str CELL (fnprops' `*const
            // u8` spelling is the same pointer).
            let etag = unsafe { __torajs_dynobj_get_tag(props, key.cast::<u8>()) };
            let eval = unsafe { __torajs_dynobj_get_value(props, key.cast::<u8>()) };
            let anyv = unsafe { __torajs_anyv_box_from_pair(etag as i64, eval as i64) };
            unsafe { __torajs_print_anyv_inline_at(anyv, indent + 2) };
            emitted += 1;
        }
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
