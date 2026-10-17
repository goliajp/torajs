//! W-J Phase C — struct-cell enumeration arms for `Object.keys`
//! (Phase C1), `Object.values` (Phase C2), and `Object.entries`
//! (Phase C3).
//!
//! tora lowers `Object.keys(o)` to a compile-time field-name literal
//! when `o`'s static type is a struct (`Type::Obj`). When `o` is
//! `any`, the struct identity is only known at runtime, so SSA-lower
//! routes here: read the instance's `class_tag` (`u32` at `+8`), look
//! the layout up in the toolchain-emitted `__torajs_class_layouts`
//! table (via the `torajs-structmeta` Phase A4 helpers), and walk the
//! field metadata in declaration order.
//!
//! ## Scope — struct cells only (narrow surface)
//!
//! This is the `Tag::Obj` static-layout slot of the W-J substrate
//! trunk. Non-struct `any` values (dynobj / array / string /
//! primitive) reflect through different substrate and are **not**
//! claimed here: rather than silently returning an empty array (which
//! would be wrong for a dynobj or array that does have keys), a
//! non-struct cell throws a loud "not yet supported" — a separate
//! trunk, tracked in plan-state L3b.
//!
//! ## Anonymous-struct caveat (A1 MVP)
//!
//! An anonymous `{a: 1}` interned too late to receive a `class_tag`
//! carries `class_tag == 0`; `__torajs_struct_layout_lookup` returns
//! NULL and the walk yields an empty array. This matches Phase B's
//! `gOPD` returning `undefined` for the same instances — a shared,
//! documented A1 anon-stamp coverage gap, not a regression introduced
//! here.

use crate::obj_own_keys_struct::{ExpandoLane, push_expando_entries, push_expando_values};
use crate::reflect::{TAG_OBJ, heap_type_tag, is_cell_imm};
use crate::struct_enum_order::{key_already_emitted, layout_field_order};
use crate::struct_reflect::slot_key;
use core::ffi::{c_char, c_void};
use value::slot_own_value;

mod value;

unsafe extern "C" {
    // torajs-structmeta (W-J Phase A4) — read-side over __torajs_class_layouts.
    fn __torajs_struct_layout_lookup(class_tag: u32) -> *const c_void;
    fn __torajs_struct_field_count(layout: *const c_void) -> u32;
    fn __torajs_struct_field_name(layout: *const c_void, idx: u32) -> StrSlice;
    fn __torajs_struct_field_info(layout: *const c_void, idx: u32) -> FieldInfo;

    // torajs-arr — build the owned result arrays. `Arr<Str>` (8-byte
    // slot) for keys; `Arr<Any>` (16-byte slot) for values.
    fn __torajs_arr_alloc(cap: u64) -> *mut u8;
    fn __torajs_arr_push(arr: *mut u8, val: i64) -> *mut u8;
    fn __torajs_arr_alloc_any(cap: u64) -> *mut u8;
    fn __torajs_arr_push_any(arr: *mut c_void, tag: u64, value: u64) -> *mut u8;

    // torajs-throw — loud failure for the non-struct slot.
    fn __torajs_throw_type_error(msg: *const c_char);
    fn __torajs_throw_check() -> i64;
    fn __torajs_arr_mark_kind(arr: *mut c_void, chain: u64);
    fn __torajs_value_drop_heap(p: *mut c_void);
}

/// `(ptr, len)` field-name slice returned by
/// `__torajs_struct_field_name` — mirrors `torajs-structmeta::StrSlice`.
#[repr(C)]
pub(crate) struct StrSlice {
    pub(crate) ptr: *const u8,
    pub(crate) len: usize,
}

/// Field byte-offset + coarse type tag returned by
/// `__torajs_struct_field_info` — mirrors `torajs-structmeta::FieldInfo`.
#[repr(C)]
struct FieldInfo {
    field_byte_offset: u32,
    type_tag: u8,
}

/// AnyValue heap tag (mirror torajs-anyvalue::AnySlotTag) — heap-cell
/// slots take an owning ref before `push_any`.
const ANY_HEAP: u64 = 4;

/// Byte offset of the `class_tag` u32 inside a `Tag::Obj` instance
/// (right after the 8-byte universal heap header).
const OBJ_CLASS_TAG_OFF: usize = 8;

/// Loud non-struct fallback message (NUL-terminated for the C throw).
/// Shared by every `Object.keys`/`values`/`entries` struct arm — the
/// non-struct `any` slot is a separate substrate trunk.
const NON_STRUCT_MSG: &[u8] = b"Object reflection on a non-struct any value is not yet supported\0";

/// A fresh rc=1 Str spelling a layout field name — `len` WTF-8 bytes
/// at `ptr` (NULL / 0 for the empty name), decoded into the cell's
/// own encoding.
#[inline]
unsafe fn alloc_str_from_raw(ptr: *const u8, len: usize) -> *mut u8 {
    let bytes = if ptr.is_null() {
        &[][..]
    } else {
        unsafe { core::slice::from_raw_parts(ptr, len) }
    };
    unsafe { crate::reflect::alloc_str_key(bytes) }
}

/// `Object.keys(v)` arm for a `Tag::Obj` struct cell. Returns an owned
/// (`+1`-rc) `Arr<Str>` of the struct's field names in declaration
/// order. A class with no layout (anonymous struct, `class_tag` 0 /
/// out of range) yields an empty array.
///
/// # Safety
/// `extern "C"` ABI. `v` is a NaN-box AnyValue immediate; for a struct
/// instance it is the raw cell pointer (cell-imm encoding). The caller
/// transfers the returned array's ownership into the SSA value flow and
/// runs a `throw_check` so the non-struct throw propagates.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_struct_keys(v: u64, include_nonenum: i64) -> *mut c_void {
    // Narrow surface: only a live `Tag::Obj` struct cell is claimed.
    if !is_cell_imm(v) || unsafe { heap_type_tag(v as *const c_void) } != TAG_OBJ {
        unsafe { __torajs_throw_type_error(NON_STRUCT_MSG.as_ptr() as *const c_char) };
        return core::ptr::null_mut();
    }
    let cell = v as *const c_void;
    let class_tag = unsafe {
        cell.cast::<u8>()
            .add(OBJ_CLASS_TAG_OFF)
            .cast::<u32>()
            .read()
    };
    let layout = unsafe { __torajs_struct_layout_lookup(class_tag) };
    // NULL layout → count 0 → empty array (A1 anon-stamp gap).
    let n = unsafe { __torajs_struct_field_count(layout) };

    let mut out = unsafe { __torajs_arr_alloc(n as u64) };
    for i in unsafe { layout_field_order(layout, n) } {
        let name = unsafe { __torajs_struct_field_name(layout, i) };
        let (kp, klen) = unsafe { slot_key(name.ptr, name.len) };
        if unsafe { error_prop_skip(cell, layout, i, (kp, klen), include_nonenum) }
            || (include_nonenum == 0 && unsafe { field_hidden_by_sidecar(cell, kp, klen) })
            || unsafe { crate::struct_field_attrs::layout_member_gone(cell, kp, klen as u32) }
        {
            continue;
        }
        if unsafe { !key_already_emitted(layout, i, (kp, klen)) } {
            let s = unsafe { alloc_str_from_raw(kp, klen) };
            out = unsafe { __torajs_arr_push(out, s as i64) };
        }
    }
    // Blade 3 (RFC 20260714-struct-dynamic-props) — fold expando
    // keys in per §10.1.11.1 (NULL props short-circuits inside).
    unsafe {
        crate::obj_own_keys_struct::struct_keys_with_expandos(
            cell,
            out as *mut c_void,
            include_nonenum,
        )
    }
}

/// Whether the enumeration walk skips this slot under the ES §20.5
/// error-instance property attributes. Three slots of the injected
/// Error layout are NOT ordinary enumerable class fields, and no two
/// of them for quite the same reason:
///
/// - `message` — §20.5.6.1.1 msgDesc `{W:1, E:0, C:1}`, and own only
///   when the ctor got one (RFC 20260718-error-message-own-prop).
/// - `stack` — the same `{W:1, E:0, C:1}` shape, but unconditionally
///   own: every construction writes a header line, so unlike
///   `message` there is no absent state to test.
/// - `name` — §20.5.3.2 puts it on `Error.prototype`, so an instance
///   owns one ONLY when user code assigned it, and such an assignment
///   is an ordinary CreateDataProperty. Its enumerability therefore
///   never moves; what moves is whether it exists, which both
///   surfaces must respect.
///
/// So the enumerable-only surfaces (`Object.keys` / for-in / values /
/// entries, `include_nonenum = 0`) skip `message` and `stack` outright,
/// the gOPN surface (`include_nonenum = 1`) keeps `stack`, and both
/// skip a slot sitting in its own-ABSENT state — the undefined
/// sentinel, which for `name` is the ordinary case.
/// Does a sidecar hide this declared field from the enumerable-only
/// surfaces (RFC 20260806-declared-field-redefine)?
///
/// The twin of [`error_prop_skip`] for attributes that USER code moved
/// rather than the spec fixing: `Object.defineProperty(c, "f",
/// {enumerable: false})`. Same shape, same three call sites, and the
/// same reason for existing — a layout field list cannot express an
/// enumerability that is not `true`.
///
/// # Safety
/// `cell` is a live `Tag::Obj` heap pointer; `kp`/`klen` name a field.
unsafe fn field_hidden_by_sidecar(cell: *const c_void, kp: *const u8, klen: usize) -> bool {
    // a replaced accessor's dict entry lists itself in the expando lane
    unsafe {
        crate::struct_field_attrs::__torajs_obj_field_is_nonenumerable(cell, kp, klen as u32) != 0
            || crate::struct_field_attrs::layout_member_gone(cell, kp, klen as u32)
    }
}

unsafe fn error_prop_skip(
    cell: *const c_void,
    layout: *const c_void,
    idx: u32,
    key: (*const u8, usize),
    include_nonenum: i64,
) -> bool {
    let (kp, klen) = key;
    let bytes = unsafe { core::slice::from_raw_parts(kp, klen) };
    let is_message = bytes == b"message";
    let is_name = bytes == b"name";
    if !is_message && !is_name && bytes != b"stack" {
        return false;
    }
    let hdr_flags = unsafe { cell.cast::<u8>().add(6).cast::<u16>().read() };
    if hdr_flags & crate::struct_reflect::FLAG_ERROR == 0 {
        return false;
    }
    let slot_absent = || {
        let info = unsafe { __torajs_struct_field_info(layout, idx) };
        let raw = unsafe {
            cell.cast::<u8>()
                .add(info.field_byte_offset as usize)
                .cast::<u64>()
                .read()
        };
        raw != 0 && unsafe { __torajs_str_is_undef(raw as *const u8) } != 0
    };
    if is_name {
        return slot_absent();
    }
    if include_nonenum == 0 {
        return true;
    }
    if !is_message {
        return false;
    }
    slot_absent()
}

unsafe extern "C" {
    /// torajs-str — undefined sentinel identity probe (RFC 20260710
    /// C1); the own-absence read of the error `message` slot.
    fn __torajs_str_is_undef(p: *const u8) -> i64;
    fn __torajs_struct_field_find(layout: *const c_void, name: *const u8, name_len: u32) -> u32;
}

/// Whether `cell` is a `FLAG_ERROR` struct whose `field` slot holds
/// the own-absence sentinel (layout self-resolved) — the
/// static-key-list filter's probe (`obj_own_keys` typed chooser).
pub(crate) unsafe fn error_field_absent(cell: *const c_void, field: &[u8]) -> bool {
    let hdr_flags = unsafe { cell.cast::<u8>().add(6).cast::<u16>().read() };
    if hdr_flags & crate::struct_reflect::FLAG_ERROR == 0 {
        return false;
    }
    let class_tag = unsafe {
        cell.cast::<u8>()
            .add(OBJ_CLASS_TAG_OFF)
            .cast::<u32>()
            .read()
    };
    let layout = unsafe { __torajs_struct_layout_lookup(class_tag) };
    if layout.is_null() {
        return false;
    }
    let idx = unsafe { __torajs_struct_field_find(layout, field.as_ptr(), field.len() as u32) };
    if idx == u32::MAX {
        return false;
    }
    let info = unsafe { __torajs_struct_field_info(layout, idx) };
    let raw = unsafe {
        cell.cast::<u8>()
            .add(info.field_byte_offset as usize)
            .cast::<u64>()
            .read()
    };
    raw != 0 && unsafe { __torajs_str_is_undef(raw as *const u8) } != 0
}

/// `Object.values(v)` arm for a `Tag::Obj` struct cell. Returns an
/// owned (`+1`-rc) `Arr<Any>` of the struct's field values in
/// declaration order, each boxed to its NaN-box AnyValue per the
/// field's coarse `type_tag` (reusing
/// `struct_reflect::field_slot_to_pair_owned`, the same decode the
/// `gOPD` value slot uses — one fresh stake per heap value so the
/// array owns its share). A class with no layout
/// (anonymous struct, `class_tag` 0 / out of range) yields an empty
/// array — the shared A1 anon-stamp gap.
///
/// # Safety
/// `extern "C"` ABI. `v` is a NaN-box AnyValue immediate; for a struct
/// instance it is the raw cell pointer. The caller transfers the
/// returned array's ownership into the SSA value flow and runs a
/// `throw_check` so the non-struct throw propagates.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_struct_values(v: u64) -> *mut c_void {
    // Narrow surface: only a live `Tag::Obj` struct cell is claimed.
    if !is_cell_imm(v) || unsafe { heap_type_tag(v as *const c_void) } != TAG_OBJ {
        unsafe { __torajs_throw_type_error(NON_STRUCT_MSG.as_ptr() as *const c_char) };
        return core::ptr::null_mut();
    }
    let cell = v as *const c_void;
    let class_tag = unsafe {
        cell.cast::<u8>()
            .add(OBJ_CLASS_TAG_OFF)
            .cast::<u32>()
            .read()
    };
    let layout = unsafe { __torajs_struct_layout_lookup(class_tag) };
    let n = unsafe { __torajs_struct_field_count(layout) };

    let mut out = unsafe { __torajs_arr_alloc_any(n as u64) };
    for i in unsafe { layout_field_order(layout, n) } {
        unsafe { push_expando_values(cell, ExpandoLane::Before(i), &mut out) };
        let name = unsafe { __torajs_struct_field_name(layout, i) };
        // A get/set pair is ONE own property: skip the second slot, the
        // same dedup the keys walk does (pre-blade-6 the values walk had
        // none, so a paired accessor contributed its value twice).
        let key = unsafe { slot_key(name.ptr, name.len) };
        if unsafe { key_already_emitted(layout, i, key) }
            || unsafe { error_prop_skip(cell, layout, i, key, 0) }
            || unsafe { field_hidden_by_sidecar(cell, key.0, key.1) }
        {
            continue;
        }
        let (tag, val) = unsafe { slot_own_value(cell, layout, i, name) };
        // the getter raised: the caller's throw edge releases nothing
        // it was handed, so the walk settles its array and hands back
        // null (the `dynobj_values_append` contract)
        if unsafe { __torajs_throw_check() } != 0 {
            unsafe { __torajs_value_drop_heap(out as *mut c_void) };
            return core::ptr::null_mut();
        }
        out = unsafe { __torajs_arr_push_any(out as *mut c_void, tag, val) };
    }
    // Blade 3 — expando values after the layout fields (insertion
    // order; a computed field rides its declaration row, 628-05).
    // Numeric-key ordering interleave is a recorded boundary: keys
    // merges integers first, values/entries append.
    unsafe {
        push_expando_values(cell, ExpandoLane::Tail, &mut out);
        push_expando_values(cell, ExpandoLane::Rest, &mut out);
    }
    out as *mut c_void
}

/// `Object.entries(v)` arm for a `Tag::Obj` struct cell. Returns an
/// owned (`+1`-rc) `Arr<Arr<Any>>` of `[name, value]` pairs in
/// declaration order — outer slot per field, each inner an
/// `Arr<Any>` of length 2 (`[name_str, boxed_value]`). Mirrors
/// `own_names::__torajs_arr_entries_by_tag`'s pair shape, with the
/// field name as the key instead of a numeric index. A class with no
/// layout yields an empty array (shared A1 anon-stamp gap).
///
/// # Safety
/// `extern "C"` ABI. `v` is a NaN-box AnyValue immediate; for a struct
/// instance it is the raw cell pointer. The caller transfers the
/// returned array's ownership into the SSA value flow and runs a
/// `throw_check` so the non-struct throw propagates.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_struct_entries(v: u64) -> *mut c_void {
    // Narrow surface: only a live `Tag::Obj` struct cell is claimed.
    if !is_cell_imm(v) || unsafe { heap_type_tag(v as *const c_void) } != TAG_OBJ {
        unsafe { __torajs_throw_type_error(NON_STRUCT_MSG.as_ptr() as *const c_char) };
        return core::ptr::null_mut();
    }
    let cell = v as *const c_void;
    let class_tag = unsafe {
        cell.cast::<u8>()
            .add(OBJ_CLASS_TAG_OFF)
            .cast::<u32>()
            .read()
    };
    let layout = unsafe { __torajs_struct_layout_lookup(class_tag) };
    let n = unsafe { __torajs_struct_field_count(layout) };

    let mut outer = unsafe { __torajs_arr_alloc(n as u64) };
    for i in unsafe { layout_field_order(layout, n) } {
        unsafe { push_expando_entries(cell, ExpandoLane::Before(i), &mut outer) };
        let name = unsafe { __torajs_struct_field_name(layout, i) };
        // The freshly minted name Str (rc=1) is consumed by push_any.
        // An accessor slot pairs under its plain property name, and its
        // value is the getter's RESULT (blade 6 — `slot_own_value`).
        let (kp, klen) = unsafe { slot_key(name.ptr, name.len) };
        // A get/set pair is ONE own property (the keys walk dedups the
        // same way).
        if unsafe { key_already_emitted(layout, i, (kp, klen)) }
            || unsafe { error_prop_skip(cell, layout, i, (kp, klen), 0) }
            || unsafe { field_hidden_by_sidecar(cell, kp, klen) }
        {
            continue;
        }
        let (tag, val) = unsafe { slot_own_value(cell, layout, i, name) };
        if unsafe { __torajs_throw_check() } != 0 {
            unsafe {
                __torajs_arr_mark_kind(
                    outer as *mut c_void,
                    crate::obj_own_keys_layout::KIND_CHAIN_HEAP,
                );
                __torajs_value_drop_heap(outer as *mut c_void);
            }
            return core::ptr::null_mut();
        }
        let name_s = unsafe { alloc_str_from_raw(kp, klen) };
        let inner = unsafe { __torajs_arr_alloc_any(2) };
        let inner = unsafe { __torajs_arr_push_any(inner as *mut c_void, ANY_HEAP, name_s as u64) };
        let inner = unsafe { __torajs_arr_push_any(inner as *mut c_void, tag, val) };
        outer = unsafe { __torajs_arr_push(outer, inner as i64) };
    }
    // Blade 3 — expando `[key, value]` pairs after the layout fields
    // (same boundary note as the values walk).
    unsafe {
        push_expando_entries(cell, ExpandoLane::Tail, &mut outer);
        push_expando_entries(cell, ExpandoLane::Rest, &mut outer);
    }
    outer as *mut c_void
}
