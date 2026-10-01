//! Giving an object cell back to the allocator from the runtime side.
//!
//! The lowering allocates every object cell and closure env as a bare
//! sized block (`__torajs_obj_alloc(32 + 8·n_fields)`) and frees it
//! with the same constant. The collector reaches the same cells
//! without that constant; a class instance recovers it from its
//! layout row — the field-metadata header carries the field count
//! the allocation was sized by — and everything else (a closure env,
//! an object whose tag never got a row) asks the span registry.

use core::ffi::c_void;

use crate::layout::{ClassLayout, is_class_obj, layout_for_class_obj};

/// Mirror of `ssa_lower::OBJ_HEADER_SIZE` — header 8 + class_tag 8 +
/// vtable 8 + props 8.
const OBJ_HEADER_SIZE: usize = 32;

unsafe extern "C" {
    #[link_name = "__torajs_free"]
    fn free_sized(p: *mut c_void, size: usize);
    #[link_name = "__torajs_free_lookup"]
    fn free_lookup(p: *mut c_void);
}

/// Head of the `.__class_fields_<i>` rodata a layout row's
/// `field_metadata_ptr` points at (torajs-link's
/// `INNER_FIELD_META_HEADER_SIZE`): the field count, then padding.
#[repr(C)]
struct FieldMetaHeader {
    n_fields: u32,
    _pad: u32,
}

/// Bytes `__torajs_obj_alloc` was asked for when it made an instance
/// of the class `lay` describes. A row with no fields leaves the
/// metadata pointer NULL.
///
/// # Safety
/// `lay` is a row of the class layout table.
unsafe fn block_size_of(lay: *const ClassLayout) -> usize {
    let meta = unsafe { (*lay).field_metadata_ptr } as *const FieldMetaHeader;
    let n_fields = if meta.is_null() {
        0
    } else {
        unsafe { (*meta).n_fields as usize }
    };
    OBJ_HEADER_SIZE + n_fields * 8
}

/// Free a class instance by the size its layout row gives.
///
/// # Safety
/// `p` satisfies `is_class_obj` and its children have already been
/// released.
pub(crate) unsafe fn free_class_cell(p: *mut c_void) {
    unsafe { free_sized(p, block_size_of(layout_for_class_obj(p))) };
}

/// Free an `__torajs_obj_alloc` block no layout row sizes — a
/// closure env, an object whose tag has no row.
///
/// # Safety
/// `p` is such a live block whose children have already been
/// released.
pub(crate) unsafe fn free_unsized_cell(p: *mut c_void) {
    unsafe { free_lookup(p) };
}

/// Free an object cell or closure env the collector owns outright.
///
/// # Safety
/// `p` is a live block from `__torajs_obj_alloc` whose children have
/// already been released.
pub(crate) unsafe fn free_obj_cell(p: *mut c_void) {
    if unsafe { is_class_obj(p) } {
        unsafe { free_class_cell(p) };
    } else {
        unsafe { free_unsized_cell(p) };
    }
}

/// [`free_obj_cell`] for the lowering: the free of a typed slot whose
/// static layout cannot size the cell it holds.
///
/// # Safety
/// As [`free_obj_cell`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_obj_free_cell(p: *mut c_void) {
    unsafe { free_obj_cell(p) };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(meta: *const c_void) -> ClassLayout {
        ClassLayout {
            n_children: 0,
            flags: 0,
            child_offsets: core::ptr::null(),
            field_metadata_ptr: meta,
            method_table_ptr: core::ptr::null(),
        }
    }

    #[test]
    fn block_size_counts_the_rows_fields() {
        let meta = FieldMetaHeader {
            n_fields: 3,
            _pad: 0,
        };
        let lay = row(&raw const meta as *const c_void);
        assert_eq!(unsafe { block_size_of(&lay) }, 32 + 3 * 8);
    }

    #[test]
    fn a_fieldless_row_is_the_bare_header() {
        let lay = row(core::ptr::null());
        assert_eq!(unsafe { block_size_of(&lay) }, 32);
    }
}
