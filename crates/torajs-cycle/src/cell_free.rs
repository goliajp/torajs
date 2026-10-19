//! Giving an object cell back to the allocator from the runtime side.
//!
//! The lowering allocates every object cell and closure env as a bare
//! sized block (`__torajs_obj_alloc(32 + 8·n_fields)`) and frees it
//! with the same constant. The collector reaches the same cells
//! without that constant: a class instance recovers it from its
//! layout row — the field-metadata header carries the field count
//! the allocation was sized by — and every `Tag::Obj` cell is a class
//! instance, since no cell is minted without a tag. A closure env that
//! died while buffered left its block size behind ([`free_parked`]).
//! Only the lowering's free of an open slot ([`__torajs_obj_free_cell`])
//! can meet a cell that is not a struct at all, and that one goes
//! back to its own tag's drop.

use core::ffi::c_void;

use crate::layout::{
    CLOSURE_DROP_FN_OFF, ClassLayout, HeapHeader, TAG_CLOSURE, is_class_obj, layout_for_class_obj,
};

/// Mirror of `ssa_lower::OBJ_HEADER_SIZE` — header 8 + class_tag 8 +
/// vtable 8 + props 8.
const OBJ_HEADER_SIZE: usize = 32;

unsafe extern "C" {
    #[link_name = "__torajs_free"]
    fn free_sized(p: *mut c_void, size: usize);
    /// torajs-value-drop — the tag-routed release of one reference.
    fn __torajs_value_drop_heap(p: *mut c_void);
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
/// `p` is a live non-literal `Tag::Obj` cell whose children have
/// already been released.
pub(crate) unsafe fn free_class_cell(p: *mut c_void) {
    unsafe { free_sized(p, block_size_of(layout_for_class_obj(p))) };
}

/// Free a cell that died while the collector held it as a candidate
/// root: its drop ran to completion, found FLAG_BUFFERED set and
/// returned without freeing, leaving the cell black at rc 0 for the
/// next root walk. Three shapes park this way. A class instance is
/// sized by its row. A closure env has no row; its drop wrote the env
/// block size over the drop-fn slot on the way out (the fn is never
/// called again), the one size record a parked env carries. An async
/// frame's drop does the same over its `desc` word.
///
/// # Safety
/// `p` is such a parked cell.
pub(crate) unsafe fn free_parked(p: *mut c_void) {
    if unsafe { (*(p as *const HeapHeader)).type_tag } == TAG_CLOSURE {
        let size = unsafe { *((p as *const u8).add(CLOSURE_DROP_FN_OFF) as *const usize) };
        unsafe { free_sized(p, size) };
    } else if unsafe { (*(p as *const HeapHeader)).type_tag } == crate::frame::TAG_ASYNC_FRAME {
        unsafe { free_sized(p, crate::frame::parked_frame_size(p)) };
    } else {
        unsafe { free_class_cell(p) };
    }
}

/// The lowering's free of a typed slot whose static layout cannot size
/// the cell it holds. A class instance is sized by its tag's row.
/// Anything else is a heap cell that is not a struct, admitted by an
/// `any` crossing — a literal promoted to a dynobj by
/// `Object.defineProperty`, say — and only its own tag's drop knows
/// what it owns: the dynobj's store block and entries, the root buffer
/// entry its runtime drop pushed while other references were alive.
/// The typed walk brought its rc to zero without releasing any of
/// that, so hand the cell the one reference its destructor consumes
/// (the collector's corpse pass does the same for shapes it does not
/// walk). A header-only free here left the buffer entry dangling into
/// the exit drain.
///
/// # Safety
/// `p` is a live block from `__torajs_obj_alloc` at rc 0 whose slot's
/// static layout has already been walked. For a foreign cell that walk
/// read the slot's offsets, not the cell's: a dynobj's fields lie
/// outside its 32-byte header and `+24` is its store, so the lowering
/// side still owes the open slot a drop by the cell's own tag.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_obj_free_cell(p: *mut c_void) {
    if unsafe { is_class_obj(p) } {
        unsafe { free_class_cell(p) };
    } else {
        unsafe {
            (*(p as *mut HeapHeader)).refcount = 1;
            __torajs_value_drop_heap(p);
        }
    }
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
