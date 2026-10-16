//! Whether a cell fits a struct slot as it is.

use core::ffi::c_void;

use torajs_rc::Tag;

use super::{
    __torajs_struct_field_count, __torajs_struct_field_info, __torajs_struct_field_name,
    __torajs_struct_layout_lookup, OBJ_CLASS_TAG_OFF, OBJ_TAG_OFF,
};
use crate::nanbox::{AnyValue, as_void_ptr, is_cell};

/// Does `av` fit a struct slot of `class_tag` as it is — a `Tag::Obj`
/// cell of that class, or of a class whose layout extends it field
/// for field (see [`cell_fits`])? 1 when it does; 0 for a non-cell, a
/// dynobj, a struct of another layout and every other heap cell. The
/// boxed adapter branches on it ahead of its unboxes: a 0 sends the
/// call to the function's all-`any` twin instead of through the
/// coercion ([`super::__torajs_anyv_arg_to_struct`]).
///
/// A slot typed by a class also asks who the cell is: the body calls
/// a method no subclass overrides as that class's own function, and
/// answers `instanceof` by the class tag, so an instance of another
/// class with the same fields would run the slot class's methods
/// (§13.3.2.1 reads `p.m` off the cell's own prototype chain). `kin`
/// then lists the tags such a cell may wear: a u32 count, then the
/// tags of the slot class, its subclasses and their generic rows
/// (unaligned rodata). A null `kin` is an interface or object-literal
/// slot, which any cell with the fields fits.
///
/// # Safety
/// `av`, when a cell, points at a live heap cell; `kin` is null or a
/// class list.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_struct_fits(
    av: AnyValue,
    class_tag: u32,
    kin: *const u8,
) -> i64 {
    if !is_cell(av) {
        return 0;
    }
    let p = as_void_ptr(av);
    unsafe {
        if (p.cast::<u8>().add(OBJ_TAG_OFF) as *const u16).read() != Tag::Obj as u16 {
            return 0;
        }
        let cell_tag = (p.cast::<u8>().add(OBJ_CLASS_TAG_OFF) as *const u32).read();
        if cell_tag == class_tag {
            return 1;
        }
        if !kin.is_null() && !kin_admits(kin, cell_tag) {
            return 0;
        }
        let layout = __torajs_struct_layout_lookup(class_tag);
        i64::from(!layout.is_null() && cell_fits(p, class_tag, layout))
    }
}

/// `cell_tag` is one of the tags of the class list `kin`.
///
/// # Safety
/// `kin` is a class list.
#[inline(always)]
pub(crate) unsafe fn kin_admits(kin: *const u8, cell_tag: u32) -> bool {
    let word = |i: usize| unsafe { kin.add(i * 4).cast::<u32>().read_unaligned() };
    (1..=word(0) as usize).any(|i| word(i) == cell_tag)
}

/// Does the `Tag::Obj` cell `p` have the layout `layout` (of
/// `class_tag`), or one that extends it field for field — same name,
/// same offset, same type — so a body reading `p` through `layout`
/// reads its own fields, and a slot of that layout can size it?
///
/// Inlined into both callers: with two of them LLVM outlines it into
/// an atom of its own, and a program that links only one caller then
/// carries a call and a prologue where it had straight-line code.
///
/// # Safety
/// `p` is a live `Tag::Obj` cell; `layout` a live lookup result.
#[inline(always)]
pub(super) unsafe fn cell_fits(p: *mut c_void, class_tag: u32, layout: *const c_void) -> bool {
    let cell_tag = unsafe { (p.cast::<u8>().add(OBJ_CLASS_TAG_OFF) as *const u32).read() };
    if cell_tag == class_tag {
        return true;
    }
    let own = unsafe { __torajs_struct_layout_lookup(cell_tag) };
    if own.is_null() {
        return false;
    }
    let n = unsafe { __torajs_struct_field_count(layout) };
    let own_n = unsafe { __torajs_struct_field_count(own) };
    if own_n < n {
        return false;
    }
    // a cell created in another order than its layout wears rows in
    // that order over the same slots, so a row that is not at the same
    // index is looked up by name
    (0..n).all(|idx| unsafe {
        same_row(layout, idx, own, idx)
            || (0..own_n).any(|j| j != idx && same_row(layout, idx, own, j))
    })
}

/// Row `i` of `a` and row `j` of `b` name the same field at the same
/// offset with the same type.
///
/// # Safety
/// `a` / `b` are live lookup results with rows `i` / `j`.
unsafe fn same_row(a: *const c_void, i: u32, b: *const c_void, j: u32) -> bool {
    let (na, nb) = unsafe {
        (
            __torajs_struct_field_name(a, i),
            __torajs_struct_field_name(b, j),
        )
    };
    na.len == nb.len
        && unsafe {
            core::slice::from_raw_parts(na.ptr, na.len)
                == core::slice::from_raw_parts(nb.ptr, nb.len)
                && __torajs_struct_field_info(a, i) == __torajs_struct_field_info(b, j)
        }
}
