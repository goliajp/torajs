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

use crate::reflect::{__torajs_rc_inc, TAG_OBJ, heap_type_tag, is_cell_imm};
use crate::struct_reflect::{accessor_slot_name, field_slot_to_pair_owned, slot_key};
use core::ffi::{c_char, c_void};

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

    // torajs-str — pooled key/name allocation.
    fn __torajs_str_alloc_pooled(len: u64) -> *mut u8;

    // torajs-throw — loud failure for the non-struct slot.
    fn __torajs_throw_type_error(msg: *const c_char);
    /// torajs-anyvalue — a struct accessor's [[Get]], keyed by raw name
    /// bytes (RFC 20260714-objlit-accessor blade 6). Answers the
    /// getter's result OWNED; `undefined` for a set-only property.
    fn __torajs_struct_accessor_get(obj: *mut c_void, name: *const u8, name_len: u32) -> u64;
    /// torajs-anyvalue — split an AnyValue into its `(tag, value)` pair.
    fn __torajs_anyv_unbox_tag(v: u64) -> i64;
    fn __torajs_anyv_unbox_value(v: u64) -> i64;
}

/// `(ptr, len)` field-name slice returned by
/// `__torajs_struct_field_name` — mirrors `torajs-structmeta::StrSlice`.
#[repr(C)]
struct StrSlice {
    ptr: *const u8,
    len: usize,
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
/// `Str` payload offset (`[header:8][len u32 @8][_pad:4][bytes @16]`).
const STR_DATA_OFF: usize = 16;

/// Loud non-struct fallback message (NUL-terminated for the C throw).
/// Shared by every `Object.keys`/`values`/`entries` struct arm — the
/// non-struct `any` slot is a separate substrate trunk.
const NON_STRUCT_MSG: &[u8] = b"Object reflection on a non-struct any value is not yet supported\0";

/// Whether an earlier slot already enumerated `key` — a `get`/`set`
/// pair occupies two layout slots but is one own property, so the
/// second one is skipped.
unsafe fn key_already_emitted(layout: *const c_void, upto: u32, key: (*const u8, usize)) -> bool {
    let mut j = 0;
    while j < upto {
        let prev = unsafe { __torajs_struct_field_name(layout, j) };
        let (p, plen) = unsafe { slot_key(prev.ptr, prev.len) };
        if plen == key.1
            && (key.1 == 0
                || unsafe {
                    core::slice::from_raw_parts(p, plen)
                        == core::slice::from_raw_parts(key.0, key.1)
                })
        {
            return true;
        }
        j += 1;
    }
    false
}

/// The `(tag, value)` an own property contributes to `Object.values` /
/// `Object.entries`, OWNED (the caller pushes it into the array, which
/// takes the ref). RFC 20260714-objlit-accessor blade 6 — an accessor
/// slot contributes the getter's RESULT ([[Get]], ES §7.3.25 / §20.1.2.x),
/// not the getter closure sitting in the slot; the walkers used to hand
/// the closure over (`[1, [Function: __getter_v]]` where bun says
/// `[1, 2]`). The getter's result arrives owned, so unlike the borrowed
/// field slot below it takes NO rc_inc.
///
/// # Safety
/// `cell` is a live `Tag::Obj` struct cell; `layout` its live entry.
unsafe fn slot_own_value(
    cell: *const c_void,
    layout: *const c_void,
    idx: u32,
    name: StrSlice,
) -> (u64, u64) {
    if let Some((_, p, plen)) = unsafe { accessor_slot_name(name.ptr, name.len) } {
        let boxed = unsafe { __torajs_struct_accessor_get(cell as *mut c_void, p, plen as u32) };
        return unsafe {
            (
                __torajs_anyv_unbox_tag(boxed) as u64,
                __torajs_anyv_unbox_value(boxed) as u64,
            )
        };
    }
    let info = unsafe { __torajs_struct_field_info(layout, idx) };
    let raw = unsafe {
        cell.cast::<u8>()
            .add(info.field_byte_offset as usize)
            .cast::<u64>()
            .read()
    };
    // Owned pair decode — exactly one fresh stake per heap payload
    // (cell +1 / an Any slot's ShortStr materializes at rc=1). The
    // pre-fix borrow-pair + rc_inc left a ShortStr's materialized
    // Str at rc=2 with one reclaiming drop — 32B leaked per
    // `Object.values` read of the slot.
    unsafe { field_slot_to_pair_owned(info.type_tag, raw) }
}

/// Allocate a pooled `Str` holding `len` bytes copied from `ptr`.
#[inline]
unsafe fn alloc_str_from_raw(ptr: *const u8, len: usize) -> *mut u8 {
    let s = unsafe { __torajs_str_alloc_pooled(len as u64) };
    if len != 0 && !ptr.is_null() {
        unsafe { core::ptr::copy_nonoverlapping(ptr, s.add(STR_DATA_OFF), len) };
    }
    s
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
    let mut i = 0;
    while i < n {
        let name = unsafe { __torajs_struct_field_name(layout, i) };
        let (kp, klen) = unsafe { slot_key(name.ptr, name.len) };
        if unsafe { error_prop_skip(cell, layout, i, (kp, klen), include_nonenum) } {
            i += 1;
            continue;
        }
        if unsafe { !key_already_emitted(layout, i, (kp, klen)) } {
            let s = unsafe { alloc_str_from_raw(kp, klen) };
            out = unsafe { __torajs_arr_push(out, s as i64) };
        }
        i += 1;
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
/// error-instance property attributes. Two slots of the injected
/// Error layout are NOT ordinary enumerable class fields:
///
/// - `message` — §20.5.6.1.1 msgDesc `{W:1, E:0, C:1}`, and own only
///   when the ctor got one (RFC 20260718-error-message-own-prop).
/// - `stack` — the same `{W:1, E:0, C:1}` shape, but unconditionally
///   own: every construction writes a header line, so unlike
///   `message` there is no absent state to test.
///
/// So the enumerable-only surfaces (`Object.keys` / for-in / values /
/// entries, `include_nonenum = 0`) skip BOTH, while the gOPN surface
/// (`include_nonenum = 1`) keeps `stack` and skips `message` only in
/// its own-ABSENT state (the undefined sentinel in the slot — no
/// ctor message / deleted).
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
    if !is_message && bytes != b"stack" {
        return false;
    }
    let hdr_flags = unsafe { cell.cast::<u8>().add(6).cast::<u16>().read() };
    if hdr_flags & crate::struct_reflect::FLAG_ERROR == 0 {
        return false;
    }
    if include_nonenum == 0 {
        return true;
    }
    if !is_message {
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

unsafe extern "C" {
    /// torajs-str — undefined sentinel identity probe (RFC 20260710
    /// C1); the own-absence read of the error `message` slot.
    fn __torajs_str_is_undef(p: *const u8) -> i64;
    fn __torajs_struct_field_find(layout: *const c_void, name: *const u8, name_len: u32) -> u32;
}

/// Whether `cell` is a `FLAG_ERROR` struct whose `message` slot holds
/// the own-absence sentinel (layout self-resolved) — the
/// static-key-list filter's probe (`obj_own_keys` typed chooser).
pub(crate) unsafe fn error_message_absent(cell: *const c_void) -> bool {
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
    let idx = unsafe { __torajs_struct_field_find(layout, b"message".as_ptr(), 7) };
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
    let mut i = 0;
    while i < n {
        let name = unsafe { __torajs_struct_field_name(layout, i) };
        // A get/set pair is ONE own property: skip the second slot, the
        // same dedup the keys walk does (pre-blade-6 the values walk had
        // none, so a paired accessor contributed its value twice).
        let key = unsafe { slot_key(name.ptr, name.len) };
        if unsafe { key_already_emitted(layout, i, key) }
            || unsafe { error_prop_skip(cell, layout, i, key, 0) }
        {
            i += 1;
            continue;
        }
        let (tag, val) = unsafe { slot_own_value(cell, layout, i, name) };
        out = unsafe { __torajs_arr_push_any(out as *mut c_void, tag, val) };
        i += 1;
    }
    // Blade 3 — expando values after the layout fields (insertion
    // order; the pair read is a borrow, the array takes a fresh
    // share). Numeric-key ordering interleave is a recorded
    // boundary: keys merges integers first, values/entries append.
    unsafe {
        crate::obj_own_keys_struct::for_each_enumerable_expando(cell, |_key, tag, val| {
            if tag == ANY_HEAP {
                __torajs_rc_inc(val as *mut c_void);
            }
            out = __torajs_arr_push_any(out as *mut c_void, tag, val);
        })
    };
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
    let mut i = 0;
    while i < n {
        let name = unsafe { __torajs_struct_field_name(layout, i) };
        // The freshly minted name Str (rc=1) is consumed by push_any.
        // An accessor slot pairs under its plain property name, and its
        // value is the getter's RESULT (blade 6 — `slot_own_value`).
        let (kp, klen) = unsafe { slot_key(name.ptr, name.len) };
        // A get/set pair is ONE own property (the keys walk dedups the
        // same way).
        if unsafe { key_already_emitted(layout, i, (kp, klen)) }
            || unsafe { error_prop_skip(cell, layout, i, (kp, klen), 0) }
        {
            i += 1;
            continue;
        }
        let name_s = unsafe { alloc_str_from_raw(kp, klen) };
        let (tag, val) = unsafe { slot_own_value(cell, layout, i, name) };
        let inner = unsafe { __torajs_arr_alloc_any(2) };
        let inner = unsafe { __torajs_arr_push_any(inner as *mut c_void, ANY_HEAP, name_s as u64) };
        let inner = unsafe { __torajs_arr_push_any(inner as *mut c_void, tag, val) };
        outer = unsafe { __torajs_arr_push(outer, inner as i64) };
        i += 1;
    }
    // Blade 3 — expando `[key, value]` pairs after the layout fields
    // (same boundary note as the values walk; both reads borrow, the
    // inner array takes fresh shares).
    unsafe {
        crate::obj_own_keys_struct::for_each_enumerable_expando(cell, |key, tag, val| {
            __torajs_rc_inc(key);
            if tag == ANY_HEAP {
                __torajs_rc_inc(val as *mut c_void);
            }
            let inner = __torajs_arr_alloc_any(2);
            let inner = __torajs_arr_push_any(inner as *mut c_void, ANY_HEAP, key as u64);
            let inner = __torajs_arr_push_any(inner as *mut c_void, tag, val);
            outer = __torajs_arr_push(outer, inner as i64);
        })
    };
    outer as *mut c_void
}
