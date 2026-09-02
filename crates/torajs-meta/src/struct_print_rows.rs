//! The rows a `Tag::Obj` struct cell contributes to a
//! `console.log` block — its layout fields, its expando entries
//! and its class prototype's methods, in bun's print order.
//!
//! Split out of [`crate::struct_print`] (rotation 563): that file
//! answers "what names this block and where does it open"; this
//! one answers "which rows go in it".

use core::ffi::c_void;

use crate::struct_print::{__torajs_inspect_line_reset, StrSlice, put_byte, put_bytes};

unsafe extern "C" {
    // torajs-structmeta (W-J Phase A4 + A3c chunk 2) — read-side
    // over __torajs_class_layouts.
    fn __torajs_struct_layout_lookup(class_tag: u32) -> *const c_void;
    fn __torajs_struct_field_count(layout: *const c_void) -> u32;
    fn __torajs_struct_field_name(layout: *const c_void, idx: u32) -> StrSlice;
    fn __torajs_struct_field_info(layout: *const c_void, idx: u32) -> FieldInfo;

    // torajs-anyvalue::inspect — indent-threaded nested-context
    // AnyValue printer (inspect indent trunk). Adds `"..."` for
    // strings, recurses into Tag::Arr / Tag::DynObj / Tag::Obj with
    // the field row's indent column.
    fn __torajs_print_anyv_inline_at(v: u64, indent: u32);
    // torajs-anyvalue::inspect — a Str key bare when it is an ASCII
    // identifier, JSON-quoted otherwise (bun's isLatin1Identifier
    // rule; the dynobj walker's key writer), a Symbol key as
    // `[Symbol(desc)]`; and the width either adds to the line.
    fn __torajs_print_str_cell_as_key(cell: *const c_void);
    fn __torajs_key_cell_print_len(cell: *const c_void) -> u32;
    fn __torajs_key_cell_inspect_hidden(key: *const c_void, flags: u64, slow: i32) -> i32;
    /// torajs-dynobj — bun's slow-walk predicate on the expando bag.
    fn __torajs_dynobj_iter_slow_mode(obj: *const c_void) -> i32;

    // torajs-anyvalue::inspect — line-width estimate primitive
    // (inspect wrap trunk), bun `estimated_line_length` mirror.
    fn __torajs_inspect_line_add(n: u32);

    // torajs-dynobj — expando-dict walk (blade 3; the own-keys
    // walker's iter trio) + entry pair read.
    fn __torajs_dynobj_iter_len(obj: *const c_void) -> u64;
    fn __torajs_dynobj_iter_key(obj: *const c_void, i: u64) -> *mut c_void;
    fn __torajs_dynobj_iter_flags(obj: *const c_void, i: u64) -> u64;
    fn __torajs_dynobj_iter_print_order(obj: *const c_void, out: *mut u64, cap: u64) -> u64;
    fn __torajs_dynobj_iter_index_count(obj: *const c_void) -> u64;
    fn __torajs_dynobj_get_tag(obj: *const c_void, key: *const u8) -> u64;
    fn __torajs_dynobj_get_value(obj: *const c_void, key: *const u8) -> u64;
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
}

/// Mirrors `torajs-structmeta::FieldInfo` — field byte offset within
/// the instance + coarse 8-bit `Type` discriminator.
#[repr(C)]
struct FieldInfo {
    field_byte_offset: u32,
    type_tag: u8,
}

#[inline]
unsafe fn put_indent(n: u32) {
    for _ in 0..n {
        unsafe { put_byte(b' ') };
    }
}
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

/// Emit a layout field name as an object key — bare when it is an
/// ASCII identifier, JSON-quoted otherwise, the same rule a dynobj
/// key takes. The name is rodata bytes, not a cell, so it is minted
/// into a Str for the shared writer and released at once.
unsafe fn put_key_bytes(ptr: *const u8, len: usize) {
    let bytes = if ptr.is_null() {
        &[][..]
    } else {
        unsafe { core::slice::from_raw_parts(ptr, len) }
    };
    let cell = unsafe { crate::reflect::alloc_str_key(bytes) };
    unsafe { __torajs_print_str_cell_as_key(cell.cast()) };
    unsafe { crate::reflect::__torajs_str_drop(cell) };
}

/// The block body of a struct cell — every row plus the `{`
/// that opens it and the `}` that closes it. `class_tag` is the
/// cell's, already read by the caller for the name prefix.
///
/// # Safety
/// `cell` is a live `Tag::Obj` cell.
pub(crate) unsafe fn put_body(cell: *const c_void, class_tag: u32, indent: u32) {
    let layout = unsafe { __torajs_struct_layout_lookup(class_tag) };
    let n = unsafe { __torajs_struct_field_count(layout) };
    // Blade 3 (RFC 20260714-struct-dynamic-props) — expando entries
    // render around the layout fields in bun's print order, every
    // own entry, enumerable or not.
    let props = unsafe { crate::obj_own_keys_struct::struct_props(cell) };
    let n_props = if props.is_null() {
        0
    } else {
        unsafe { __torajs_dynobj_iter_len(props) }
    };

    // 405-05 — a method-carrying class prints its prototype entries
    // even with zero own properties (`M { m: [Function: m] }`).
    let has_methods = unsafe { crate::struct_print_methods::has_visible_methods(class_tag) };
    if n == 0 && n_props == 0 && !has_methods {
        unsafe { put_bytes(b"{}") };
        return;
    }

    unsafe { put_bytes(b"{\n") };
    // bun handleFirstProperty estimate seed — see
    // torajs-dynobj::print_any for the accumulation rationale.
    unsafe { __torajs_inspect_line_reset(indent + 1) };
    // Expando entries in bun's print order (see dynobj
    // `iter_print_order`); the leading `n_idx` are array-index keys,
    // which bun prints before the layout fields (the butterfly comes
    // before the structure's properties), the rest after them.
    let mut order = vec![0u64; n_props as usize];
    let n_ord = if n_props > 0 {
        unsafe { __torajs_dynobj_iter_print_order(props, order.as_mut_ptr(), n_props) }
    } else {
        0
    };
    order.truncate(n_ord as usize);
    let n_idx = if n_props > 0 {
        unsafe { __torajs_dynobj_iter_index_count(props) as usize }
    } else {
        0
    };
    // bun's fast / slow walk (anyvalue `key_hidden` module doc): slow
    // when the expando bag rules the fast walk out, or when nothing
    // at all would print under it (no layout field, no prototype
    // method, no visible expando — bun's `anyHits` restart).
    let mut slow = if props.is_null() {
        0
    } else {
        unsafe { __torajs_dynobj_iter_slow_mode(props) }
    };
    if slow == 0 && n == 0 && !has_methods {
        let fast_hit = order.iter().any(|&pi| unsafe {
            let key = __torajs_dynobj_iter_key(props, pi);
            !key.is_null()
                && __torajs_key_cell_inspect_hidden(key, __torajs_dynobj_iter_flags(props, pi), 0)
                    == 0
        });
        if !fast_hit && !order.is_empty() {
            slow = 1;
        }
    }
    // Accessor slots collapse a get/set pair into one entry, so the
    // separator keys off what was actually emitted, not off `i`.
    let mut emitted: u32 = 0;
    for &pi in &order[..n_idx] {
        unsafe { put_expando_row(props, pi, indent, &mut emitted, slow) };
    }
    let mut i: u32 = 0;
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
        // An own `constructor` is the one key bun's inspect always
        // leaves out (`key_hidden`); a literal's declared field of
        // that name is one too.
        if unsafe { core::slice::from_raw_parts(kp, klen) } == b"constructor" {
            i += 1;
            continue;
        }
        if emitted > 0 {
            unsafe { put_bytes(b",\n") };
            unsafe { __torajs_inspect_line_add(1) };
        }
        unsafe { put_indent(indent + 2) };
        unsafe { put_key_bytes(kp, klen) };
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
    // Blade 3 — the string- and Symbol-keyed expando entries render
    // after the layout fields.
    for &pi in &order[n_idx..] {
        unsafe { put_expando_row(props, pi, indent, &mut emitted, slow) };
    }
    // 405-05 — prototype methods and accessors render after the own
    // properties, matching bun's inspect order (own first, proto
    // entries last; see struct_print_methods.rs).
    if has_methods {
        unsafe {
            crate::struct_print_methods::print_proto_methods(class_tag, indent, &mut emitted)
        };
    }
    unsafe { put_bytes(b",\n") };
    unsafe { __torajs_inspect_line_add(1) };
    unsafe { put_indent(indent) };
    unsafe { put_byte(b'}') };
}

/// One expando entry row — `key: value`, enumerable or not; the pair
/// read is a borrow, boxed for the inline printer with zero rc
/// traffic. `emitted` drives the separator like the field rows.
///
/// # Safety
/// `props` is the live expando dynobj of the struct being printed
/// and `pi` a dense-entry index from its print order.
unsafe fn put_expando_row(
    props: *const c_void,
    pi: u64,
    indent: u32,
    emitted: &mut u32,
    slow: i32,
) {
    let key = unsafe { __torajs_dynobj_iter_key(props, pi) };
    if key.is_null() {
        return;
    }
    let flags = unsafe { __torajs_dynobj_iter_flags(props, pi) };
    if unsafe { __torajs_key_cell_inspect_hidden(key, flags, slow) } != 0 {
        return;
    }
    if *emitted > 0 {
        unsafe { put_bytes(b",\n") };
        unsafe { __torajs_inspect_line_add(1) };
    }
    // Line accounting counts the key's code units (bun adds
    // str.length); the key itself goes through the shared key
    // writer, like a dynobj key (a Symbol expando prints as
    // `[Symbol(desc)]`).
    let klen = unsafe { __torajs_key_cell_print_len(key) };
    unsafe { put_indent(indent + 2) };
    unsafe { __torajs_print_str_cell_as_key(key) };
    unsafe { put_bytes(b": ") };
    unsafe { __torajs_inspect_line_add(klen + 2) };
    // The kernel keys by the live Str CELL (fnprops' `*const u8`
    // spelling is the same pointer).
    let etag = unsafe { __torajs_dynobj_get_tag(props, key.cast::<u8>()) };
    let eval = unsafe { __torajs_dynobj_get_value(props, key.cast::<u8>()) };
    let anyv = unsafe { __torajs_anyv_box_from_pair(etag as i64, eval as i64) };
    unsafe { __torajs_print_anyv_inline_at(anyv, indent + 2) };
    *emitted += 1;
}
