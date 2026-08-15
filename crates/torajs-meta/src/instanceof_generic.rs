//! 405-03 — `instanceof` against a GENERIC class.
//!
//! A generic class's instances live on per-specialization sids, each
//! wearing its own anon-pool tag (404-01), so the compile-time
//! descendant-tag equality chain — constants baked at the instanceof
//! site — can never list them: the tags are minted while Pass 2
//! lowers the mono factories, in no fixed order relative to the
//! instanceof site. What every specialization row DOES carry is the
//! class's name (404-01 dresses it in), and a class name is
//! program-unique by the time it reaches a row (top-level collisions
//! are α-renamed by the hoist, and a nested generic class never
//! lowers — the capturing lane declines type parameters). So the
//! runtime half of the answer is name identity: the receiver's row
//! and the target class's row naming the same class means the
//! receiver is one of its specializations.
//!
//! A tag with no name-table entry (a plain anonymous struct) answers
//! a NULL slice and matches nothing.

use core::ffi::c_void;

use crate::reflect::{TAG_OBJ, heap_type_tag, is_cell_imm};

/// Mirrors `torajs-structmeta`'s return shape (same as
/// `struct_print`'s local mirror).
#[repr(C)]
struct StrSlice {
    ptr: *const u8,
    len: usize,
}

unsafe extern "C" {
    fn __torajs_struct_class_name(class_tag: u32) -> StrSlice;
}

/// Universal object header: class_tag u32 at +8.
const OBJ_CLASS_TAG_OFF: usize = 8;

pub(crate) fn names_match(tag: u32, base_tag: u32) -> bool {
    if tag == 0 || base_tag == 0 {
        return false;
    }
    if tag == base_tag {
        return true;
    }
    // SAFETY: the name table answers a NULL/0 slice for an unknown
    // tag; a non-NULL slice points at rodata name bytes.
    unsafe {
        let a = __torajs_struct_class_name(tag);
        let b = __torajs_struct_class_name(base_tag);
        if a.ptr.is_null() || b.ptr.is_null() || a.len == 0 || a.len != b.len {
            return false;
        }
        core::slice::from_raw_parts(a.ptr, a.len) == core::slice::from_raw_parts(b.ptr, b.len)
    }
}

/// Typed-receiver half: the lowering already loaded the receiver's
/// class tag, so only the name comparison is left.
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_generic_tag_match(tag: i64, base_tag: i64) -> bool {
    if !(0..=u32::MAX as i64).contains(&tag) || !(0..=u32::MAX as i64).contains(&base_tag) {
        return false;
    }
    names_match(tag as u32, base_tag as u32)
}

/// Any-receiver half: decode the cell, gate on a struct instance,
/// read its tag, compare names.
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_instanceof_generic_any(v: u64, base_tag: i64) -> bool {
    if !is_cell_imm(v) {
        return false;
    }
    let p = v as usize as *const c_void;
    // SAFETY: a cell-encoded AnyValue is a live heap pointer with the
    // universal header; TAG_OBJ instances carry class_tag at +8.
    unsafe {
        if heap_type_tag(p) != TAG_OBJ {
            return false;
        }
        let tag = p.cast::<u8>().add(OBJ_CLASS_TAG_OFF).cast::<u32>().read();
        __torajs_generic_tag_match(tag as i64, base_tag)
    }
}
