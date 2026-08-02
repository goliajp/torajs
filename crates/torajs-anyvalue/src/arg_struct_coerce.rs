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
//! an OWNED AnyValue whose payload the body can safely read through
//! the layout:
//!
//! - non-cell values pass through untouched (the adapter's rc_dec
//!   release no-ops on immediates — behavior identical to before);
//! - a `Tag::Obj` cell (already the right repr) takes a +1 stake;
//! - a `Tag::DynObj` cell materializes a FRESH struct cell: per
//!   layout field, read the dynobj entry by name and convert to the
//!   slot's repr (Any slots re-box with a stake; number/bool slots
//!   coerce; Str slots go through ToString — an absent field's
//!   `undefined` stringifies to "undefined", matching what the JS
//!   concat the body performs would print; other heap slots take
//!   the pointer with a stake, or NULL when absent);
//! - every other cell tag takes a +1 stake and passes through (the
//!   pre-kernel behavior, kept loud-or-identical).
//!
//! Accessor entries (`ANY_ACCESSOR_TAG`) are out of scope — an
//! accessor-bearing literal as a typed-destr argument has no test262
//! presence; the slot reads as its stored pair bits would before.
//!
//! The adapter releases the answer after the body call with
//! `__torajs_anyv_rc_dec` — balancing the stake / freeing the fresh
//! cell through the universal heap drop (per-field walk).

use core::ffi::c_void;

use torajs_rc::Tag;

use crate::nanbox::{AnyValue, as_void_ptr, is_cell};

/// Mirror of `torajs-structmeta::StrSlice` (repr(C) pair).
#[repr(C)]
struct StrSlice {
    ptr: *const u8,
    len: usize,
}

/// Mirror of `torajs-structmeta::FieldInfo`.
#[repr(C)]
struct FieldInfo {
    field_byte_offset: u32,
    type_tag: u8,
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
}

const ANY_HEAP: u64 = 4;
const OBJ_TAG_OFF: usize = 4;
const OBJ_CLASS_TAG_OFF: usize = 8;
/// Mirror of `ssa_lower::OBJ_HEADER_SIZE` (blade 1: 32 — header 8 +
/// class_tag 8 + vtable 8 + props dynobj 8). `alloc_zeroed` below
/// zeroes the +24 props slot along with the vtable.
const OBJ_HEADER_SIZE: usize = 32;

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
        if tag != Tag::DynObj as u16 {
            __torajs_rc_inc(p);
            return av;
        }
        let layout = __torajs_struct_layout_lookup(class_tag);
        if layout.is_null() {
            // Pass-2-interned sid with no baked layout row — graceful
            // degradation, the pre-kernel passthrough.
            __torajs_rc_inc(p);
            return av;
        }
        let n = __torajs_struct_field_count(layout);
        let size = OBJ_HEADER_SIZE + (n as usize) * 8;
        let cell = std::alloc::alloc_zeroed(core::alloc::Layout::from_size_align(size, 8).unwrap());
        *(cell as *mut u32) = 1;
        *(cell.add(OBJ_TAG_OFF) as *mut u16) = Tag::Obj as u16;
        *(cell.add(OBJ_CLASS_TAG_OFF) as *mut u32) = class_tag;
        for idx in 0..n {
            let name = __torajs_struct_field_name(layout, idx);
            let info = __torajs_struct_field_info(layout, idx);
            if info.field_byte_offset == 0 {
                continue;
            }
            let key = __torajs_str_alloc(name.ptr, name.len as i64);
            let dtag = __torajs_dynobj_get_tag(p as *const c_void, key as *const c_void);
            let dval = __torajs_dynobj_get_value(p as *const c_void, key as *const c_void);
            __torajs_str_drop(key as *mut c_void);
            let entry_box =
                crate::nanbox_encode::__torajs_anyv_box_from_pair(dtag as i64, dval as i64);
            let slot: u64 = match info.type_tag {
                // Any slot — the box itself, with a stake on heap
                // payloads (the dynobj get is a borrow).
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
