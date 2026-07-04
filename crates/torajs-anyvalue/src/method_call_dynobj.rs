//! `Tag::DynObj` + `Tag::Obj` receiver arms for
//! `__torajs_any_method_call` (split out of `method_call.rs` — the
//! dispatcher stays the id-switch, the property-probing receivers
//! live here).
//!
//! - `dynobj_method` — probe the property by the interned name Str;
//!   a closure-cell value with a boxed dual entry invokes through
//!   the uniform ABI; an accessor entry's getter runs first and its
//!   answer dispatches as the callee (C4+ chunk 523).
//! - `struct_method` (L3b #9, chunk 524) — static-layout class /
//!   anonymous-struct instances (`Tag::Obj`) store fields at fixed
//!   byte offsets, so the probe walks the toolchain-emitted
//!   `__torajs_class_layouts` table (torajs-structmeta W-J Phase A4
//!   read side): class_tag@+8 → layout lookup → field find by the
//!   name Str's bytes → the slot's static type decides the callee
//!   shape. A `Closure`-typed slot (field type_tag 8) holds the raw
//!   env cell; an `Any`-typed slot (tag 0) holds a NaN-box that may
//!   wrap one. Everything else — no layout (class_tag 0), absent
//!   field, non-callable slot type — answers the same catchable
//!   TypeError as the other arms, never a layout mis-read. This is
//!   what makes `n.inner.op(1, 1)` work when `inner: { op: (a, b) =>
//!   a + b }` nests inside an `any` ObjectLit init (the nested
//!   literal lowers as an anon struct, not a dynobj).
//!
//! Argument ledger: identical to the dispatcher — the property
//! probe borrows (a struct field slot is owned by the receiver,
//! which outlives the call); the adapter's return is caller-owned
//! per the boxed-value convention.

use core::ffi::c_void;

use crate::method_call::{closure_boxed_entry, closure_cell_entry, method_not_a_function};
use crate::nanbox::AnyValue;

unsafe extern "C" {
    /// torajs-dynobj — property probe by Str key: the slot's ANY_TAG
    /// (5 = absent) / per-tag payload.
    fn __torajs_dynobj_get_tag(obj: *const c_void, key: *const c_void) -> u64;
    fn __torajs_dynobj_get_value(obj: *const c_void, key: *const c_void) -> u64;
    /// torajs-dynobj — run an accessor entry's getter; the answer is
    /// an owned AnyValue per the boxed-value convention.
    fn __torajs_accessor_invoke_getter(pair: *const c_void) -> u64;
    /// torajs-structmeta — read side over `__torajs_class_layouts`
    /// (NULL for class_tag 0 / past the table).
    fn __torajs_struct_layout_lookup(class_tag: u32) -> *const c_void;
    /// torajs-structmeta — field index by name bytes (u32::MAX miss).
    fn __torajs_struct_field_find(layout: *const c_void, name: *const u8, name_len: u32) -> u32;
    /// torajs-structmeta — field byte offset + coarse type tag
    /// (zeroed for a miss; a real offset is never 0).
    fn __torajs_struct_field_info(layout: *const c_void, idx: u32) -> FieldInfo;
}

/// Mirror of `torajs-structmeta::FieldInfo` (returned by value
/// across the FFI).
#[repr(C)]
struct FieldInfo {
    field_byte_offset: u32,
    type_tag: u8,
}

/// Accessor-entry sentinel in the dynobj probe's tag channel —
/// mirror of torajs-core `ssa_lower_accessor.rs::ANY_ACCESSOR_TAG`.
const ANY_ACCESSOR_TAG: u64 = 6;

/// Byte offset of the `class_tag` u32 inside a `Tag::Obj` instance —
/// mirror of torajs-core `ssa_lower::OBJ_CLASS_TAG_OFF`.
const OBJ_CLASS_TAG_OFF: usize = 8;

/// Interned name Str layout — mirror of torajs-str
/// `layout::{STR_LEN_OFF, STR_DATA_OFF}`.
const STR_LEN_OFF: usize = 8;
const STR_DATA_OFF: usize = 16;

/// Field type_tags whose slot can hold a callee — mirror of
/// torajs-core `ssa::module_methods::field_type_tag_of` (0 = Any,
/// 8 = Closure).
const FIELD_TAG_ANY: u8 = 0;
const FIELD_TAG_CLOSURE: u8 = 8;

/// `Tag::DynObj` arm — probe the property by name; a closure-cell
/// value invokes through its boxed dual entry. The property probe
/// borrows (dynobj keeps its own value reference; the receiver
/// outlives the call), and the adapter's return is caller-owned per
/// the boxed-value convention.
pub(crate) unsafe fn dynobj_method(
    obj: *mut c_void,
    name_str: *const u8,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    unsafe {
        if !name_str.is_null() {
            let key = name_str as *const c_void;
            let dtag = __torajs_dynobj_get_tag(obj, key);
            // ANY_HEAP = 4 — a plain closure-cell property.
            if dtag == 4 {
                let cell = __torajs_dynobj_get_value(obj, key);
                // The cell's NaN-box encoding is its pointer bits.
                if let Some((env, entry)) = closure_boxed_entry(cell) {
                    return crate::method_call::invoke_boxed(env, entry, argv, argc);
                }
            }
            // C4+ chunk 523 — getter-as-callee: an accessor entry's
            // getter runs first, its (owned) answer dispatches as
            // the callee, and the reference releases after the call
            // (the invoke keeps the cell alive across it).
            if dtag == ANY_ACCESSOR_TAG {
                let pair = __torajs_dynobj_get_value(obj, key) as *const c_void;
                let got = __torajs_accessor_invoke_getter(pair);
                if let Some((env, entry)) = closure_boxed_entry(got) {
                    let r = crate::method_call::invoke_boxed(env, entry, argv, argc);
                    crate::nanbox_ffi::__torajs_anyv_rc_dec(got);
                    return r;
                }
                crate::nanbox_ffi::__torajs_anyv_rc_dec(got);
            }
        }
        method_not_a_function()
    }
}

/// `Tag::Obj` arm — see module doc. `obj` is the struct cell, and
/// the field slot read is a borrow (the receiver owns it and
/// outlives the call).
pub(crate) unsafe fn struct_method(
    obj: *mut c_void,
    name_str: *const u8,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    unsafe {
        if !name_str.is_null() {
            let class_tag = (obj.cast::<u8>().add(OBJ_CLASS_TAG_OFF) as *const u32).read();
            let layout = __torajs_struct_layout_lookup(class_tag);
            if !layout.is_null() {
                let name_len = (name_str.add(STR_LEN_OFF) as *const u32).read();
                let name_bytes = name_str.add(STR_DATA_OFF);
                let idx = __torajs_struct_field_find(layout, name_bytes, name_len);
                if idx != u32::MAX {
                    let info = __torajs_struct_field_info(layout, idx);
                    let raw = (obj.cast::<u8>().add(info.field_byte_offset as usize) as *const u64)
                        .read();
                    let pair = match info.type_tag {
                        // The slot is itself a NaN-box AnyValue.
                        FIELD_TAG_ANY => closure_boxed_entry(raw),
                        // The slot is the raw closure env cell.
                        FIELD_TAG_CLOSURE => closure_cell_entry(raw as *mut c_void),
                        _ => None,
                    };
                    if let Some((env, entry)) = pair {
                        return crate::method_call::invoke_boxed(env, entry, argv, argc);
                    }
                }
            }
        }
        method_not_a_function()
    }
}
