//! S2.36 — boxed-adapter struct-param coercion kernel.
//!
//! An any-dispatched call packs an INLINE object-literal argument as
//! a dynobj box (the any-lane objlit define/expando contract), while
//! the boxed adapter of a typed body unboxed every heap param as raw
//! pointer bits and the body then read it through its STRUCT layout
//! — garbage on every field, and the whole process died silently
//! (rotation 238 probes rg4-rg10: `(new B() as any).named({inline})`
//! where `named({first, last}: …)` exited 0 with no output; a
//! pre-bound typed-struct argument round-tripped fine).
//!
//! This kernel is the repr boundary: the adapter calls it per
//! `Type::Obj(sid)` param with the sid's class_tag, and it answers
//! an OWNED AnyValue whose payload the body can read through the
//! layout, and — what the typed drops behind the body's slots
//! depend on — whose cell, if it is one, has that layout or one
//! extending it by prefix:
//!
//! - non-cell values pass through untouched (the adapter's rc_dec
//!   release no-ops on immediates — behavior identical to before);
//! - a `Tag::Obj` cell of the parameter's class, or of a class whose
//!   layout extends the parameter's field for field (a subclass
//!   instance, which the typed side already admits), takes a +1
//!   stake and passes through;
//! - a `Tag::Obj` cell of any other layout, or a `Tag::DynObj` cell,
//!   materializes a FRESH struct cell: per layout field, read the
//!   source's entry by name — a struct cell's declared field through
//!   its own layout row, a dynobj's entry — and convert to the slot's
//!   repr (Any slots re-box with a stake; number/bool slots coerce;
//!   Str slots go through ToString — an absent field's `undefined`
//!   stringifies to "undefined", matching what the JS concat the body
//!   performs would print; other heap slots take the pointer with a
//!   stake, or NULL when absent);
//! - every other heap cell (a Proxy standing in for the receiver, a
//!   string, an array) takes a +1 stake and passes through: the body
//!   may compare it by identity (`this === proxy`), and a copy would
//!   not be it. Such a cell is not a struct, so a struct-typed slot
//!   cannot size it; a body that stores one into such a slot is the
//!   remaining hole of this boundary.
//!
//! Accessor entries (`ANY_ACCESSOR_TAG`) are out of scope — an
//! accessor-bearing literal as a typed-destr argument has no test262
//! presence; the slot reads as its stored pair bits would before.
//!
//! The adapter releases the answer after the body call with
//! `__torajs_anyv_rc_dec` — balancing the stake / freeing the fresh
//! cell through the universal heap drop (per-field walk).
//!
//! An adapter whose function has an all-`any` twin asks
//! [`fits::__torajs_anyv_struct_fits`] first and hands the whole call to
//! the twin when a struct slot's argument does not fit: there the
//! parameter is read through the generic [[Get]] (§13.3.2.1), which
//! is what an array, a Proxy, a getter-bearing literal or a `3`
//! means for `p.x`, and the argument keeps its identity (§10.2.1.3).
//! A slot that fits passes its cell through under the adapter's own
//! stake, without this kernel. The materializing arm above then only
//! serves the targets no twin reaches (a nested closure escaping
//! through `any`, a receiver or argv channel).

use core::ffi::c_void;

use torajs_rc::{AnySlotTag, Tag};

use crate::nanbox::{AnyValue, as_void_ptr, is_cell};
use crate::struct_probe::struct_field_pair_bytes;

mod fits;
use fits::cell_fits;
pub(crate) use fits::{__torajs_anyv_struct_fits, kin_admits};

/// Mirror of `torajs-structmeta::StrSlice` (repr(C) pair).
#[repr(C)]
pub(crate) struct StrSlice {
    pub(crate) ptr: *const u8,
    pub(crate) len: usize,
}

/// Mirror of `torajs-structmeta::FieldInfo`.
#[repr(C)]
#[derive(PartialEq, Eq)]
pub(crate) struct FieldInfo {
    pub(crate) field_byte_offset: u32,
    pub(crate) type_tag: u8,
}

unsafe extern "C" {
    fn __torajs_struct_layout_lookup(class_tag: u32) -> *const c_void;
    fn __torajs_struct_field_count(layout: *const c_void) -> u32;
    fn __torajs_struct_field_name(layout: *const c_void, idx: u32) -> StrSlice;
    fn __torajs_struct_field_info(layout: *const c_void, idx: u32) -> FieldInfo;
    fn __torajs_dynobj_get_tag(obj: *const c_void, key: *const c_void) -> u64;
    fn __torajs_dynobj_get_value(obj: *const c_void, key: *const c_void) -> u64;
    fn __torajs_str_alloc(p: *const u8, len: i64) -> *mut u8;
    fn __torajs_str_drop(p: *mut c_void);
    fn __torajs_rc_inc(p: *mut c_void);
    /// torajs-mmalloc zeroed sized alloc — the same allocator and the
    /// same bare block shape as the lowering's `__torajs_obj_alloc`,
    /// so the universal drop can free it by size.
    #[link_name = "__torajs_calloc"]
    fn calloc(size: usize) -> *mut u8;
}

const ANY_HEAP: u64 = 4;
pub(crate) const OBJ_TAG_OFF: usize = 4;
pub(crate) const OBJ_CLASS_TAG_OFF: usize = 8;
/// Mirror of `ssa_lower::OBJ_HEADER_SIZE` (blade 1: 32 — header 8 +
/// class_tag 8 + vtable 8 + props dynobj 8). `calloc` below zeroes
/// the +24 props slot along with the vtable.
pub(crate) const OBJ_HEADER_SIZE: usize = 32;

/// See the module doc. `class_tag` is the param sid's stamp (anon
/// pool / class factory — the same tag the ObjectLit alloc writes at
/// `+8`), which keys the toolchain-emitted layout table.
///
/// # Safety
/// `av`, when a cell, points at a live heap cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_arg_to_struct(av: AnyValue, class_tag: u32) -> AnyValue {
    unsafe {
        if !is_cell(av) {
            return av;
        }
        let p = as_void_ptr(av);
        let tag = (p.cast::<u8>().add(OBJ_TAG_OFF) as *const u16).read();
        let layout = __torajs_struct_layout_lookup(class_tag);
        if layout.is_null() {
            // Pass-2-interned sid with no baked layout row — graceful
            // degradation, the pre-kernel passthrough.
            __torajs_rc_inc(p);
            return av;
        }
        let foreign_struct = tag == Tag::Obj as u16 && !cell_fits(p, class_tag, layout);
        if !foreign_struct && tag != Tag::DynObj as u16 {
            __torajs_rc_inc(p);
            return av;
        }
        let n = __torajs_struct_field_count(layout);
        let size = OBJ_HEADER_SIZE + (n as usize) * 8;
        let cell = calloc(size);
        *(cell as *mut u32) = 1;
        *(cell.add(OBJ_TAG_OFF) as *mut u16) = Tag::Obj as u16;
        *(cell.add(OBJ_CLASS_TAG_OFF) as *mut u32) = class_tag;
        for idx in 0..n {
            let name = __torajs_struct_field_name(layout, idx);
            let info = __torajs_struct_field_info(layout, idx);
            if info.field_byte_offset == 0 {
                continue;
            }
            let (dtag, dval) =
                source_entry(p, tag, core::slice::from_raw_parts(name.ptr, name.len));
            let entry_box =
                crate::nanbox_encode::__torajs_anyv_box_from_pair(dtag as i64, dval as i64);
            let slot: u64 = match info.type_tag {
                // Any slot — the box itself, with a stake on heap
                // payloads (the source read is a borrow).
                0 => {
                    if dtag == ANY_HEAP {
                        __torajs_rc_inc(dval as *mut c_void);
                    }
                    entry_box
                }
                1 => crate::nanbox_ffi::__torajs_anyv_to_number(entry_box) as i64 as u64,
                2 => crate::nanbox_ffi::__torajs_anyv_to_number(entry_box).to_bits(),
                3 => u64::from(crate::nanbox_ffi::__torajs_anyv_to_bool(entry_box)),
                // Str slot — ToString hands back an owned cell the
                // slot keeps (absent fields stringify "undefined",
                // the same text the body's concat would produce).
                4 => crate::nanbox_ffi::__torajs_anyv_to_str(entry_box) as u64,
                // Heap-typed slots — pointer with a stake; absent or
                // non-heap entries leave the zeroed NULL.
                _ => {
                    if dtag == ANY_HEAP {
                        __torajs_rc_inc(dval as *mut c_void);
                        dval
                    } else {
                        0
                    }
                }
            };
            (cell.add(info.field_byte_offset as usize) as *mut u64).write(slot);
        }
        crate::nanbox_encode::__torajs_anyv_box_from_pair(ANY_HEAP as i64, cell as i64)
    }
}

/// The `(any_tag, payload)` pair a layout field named `name` takes
/// from the source cell `p` of type `tag`: a struct cell's declared
/// field, or a dynobj's entry; `undefined` when absent. Borrow-shaped:
/// the source keeps its stake.
///
/// # Safety
/// `p` is a live `Tag::Obj` or `Tag::DynObj` cell whose type tag is
/// `tag`.
unsafe fn source_entry(p: *mut c_void, tag: u16, name: &[u8]) -> (u64, u64) {
    if tag == Tag::Obj as u16 {
        let absent = (AnySlotTag::Undef as u64, 0);
        return unsafe { struct_field_pair_bytes(p, name) }.unwrap_or(absent);
    }
    unsafe {
        let key = __torajs_str_alloc(name.as_ptr(), name.len() as i64);
        let dtag = __torajs_dynobj_get_tag(p, key as *const c_void);
        let dval = __torajs_dynobj_get_value(p, key as *const c_void);
        __torajs_str_drop(key as *mut c_void);
        (dtag, dval)
    }
}
