//! The boxed adapter's layout test for an array-of-struct slot.
//!
//! A typed body given `toks: Tok[]` indexes the array and reads each
//! element through `Tok`'s layout — sound only for a typed array of
//! cells of that layout or of one extending it field for field. The
//! adapter of such a body asks this kernel ahead of its unbox, the way
//! it asks `__torajs_anyv_struct_fits` for a plain struct slot, and
//! hands a call whose argument does not fit to the body's all-`any`
//! twin, where `toks[i].value` is the generic [[Get]] (§13.3.2.1) and
//! `null.length` the TypeError §13.3.2.1 step 1 asks for.
//!
//! Fits: a `Tag::Arr` cell with raw heap-pointer slots
//! (`ARR_KIND_HEAP`, stamped when the array crossed into `any`) and
//! no exotic index or sparse tail, every live slot of which is a cell
//! `__torajs_anyv_struct_fits` admits — behind `depth - 1` more levels
//! of such arrays for a nested slot type. Everything else does not: an
//! `Array<Any>` block (boxes, not pointers), an unstamped or
//! scalar-kind block, a NULL slot (a `(Tok | null)[]`, whose `.value`
//! read is the TypeError §13.3.2.1 asks for, not a typed null
//! dereference), a string, a `null`, an `undefined`.

use torajs_rc::{
    ARR_KIND_HEAP, FLAG_ARR_ANY, FLAG_ARR_EXOTIC_INDEX, FLAG_ARR_SPARSE_TAIL, HeapHeader, Tag,
};

use crate::grow::ARR_HDR_HEAD_OFF;
use crate::layout::{ARR_LEN_OFF, arr_data};

unsafe extern "C" {
    /// A heap cell's pointer, 0 for every immediate.
    fn __torajs_anyv_cell_ptr(v: u64) -> i64;
    /// 1 when the cell has the class's layout or one extending it.
    fn __torajs_anyv_struct_fits(v: u64, class_tag: u32) -> i64;
}

/// `__torajs_anyv_arr_fits(v, class_tag, depth)` — see the module doc.
/// 1 when `v` is an array that fits a slot of `depth` levels of array
/// over cells of `class_tag`; 0 otherwise.
///
/// # Safety
/// `v` is a valid NaN-box; a cell it names is live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_arr_fits(v: u64, class_tag: u32, depth: i64) -> i64 {
    let cell = unsafe { __torajs_anyv_cell_ptr(v) } as *const u8;
    i64::from(!cell.is_null() && unsafe { arr_fits(cell, class_tag, depth) })
}

/// `__torajs_anyv_arr_cell(v, chain, str_leaf)` — 1 when `v` is an
/// array block a typed body over non-struct elements can index as it
/// is. The adapter of such an array slot asks it ahead of its pointer
/// unbox and hands every other argument to the all-`any` twin, where
/// `xs.length` on `null` is the TypeError §13.3.2.1 asks for and an
/// element is read through the generic [[Get]].
///
/// `chain` is the slot's element-kind chain, 3 bits per level, the
/// encoding `__torajs_arr_mark_kind` stamps. A level of kind 0 is an
/// `any[]` slot, whose body reads every block kind-aware, so any
/// `Tag::Arr` cell fits it. Any other level needs a block stamped with
/// exactly that kind: an `Array<Any>` block holds boxes, not raw
/// slots, an unstamped or differently stamped block holds another
/// representation, and an exotic index or a sparse tail is not in the
/// slots at all. A heap level with a further level walks every element
/// as an array of the next level; with `str_leaf`, the innermost
/// elements must be `Tag::Str` cells.
///
/// # Safety
/// `v` is a valid NaN-box; a cell it names is live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_arr_cell(v: u64, chain: u64, str_leaf: i64) -> i64 {
    let cell = unsafe { __torajs_anyv_cell_ptr(v) } as *const u8;
    i64::from(!cell.is_null() && unsafe { arr_cell_fits(cell, chain, str_leaf != 0) })
}

/// # Safety
/// `arr` is a live heap cell.
unsafe fn arr_cell_fits(arr: *const u8, chain: u64, str_leaf: bool) -> bool {
    const UNFIT: u16 = FLAG_ARR_ANY | FLAG_ARR_EXOTIC_INDEX | FLAG_ARR_SPARSE_TAIL;
    let hdr = unsafe { &*(arr as *const HeapHeader) };
    if hdr.tag() != Tag::Arr {
        return false;
    }
    let kind = (chain & 0b111) as u16;
    if kind == 0 {
        return true;
    }
    if hdr.flags & UNFIT != 0 || hdr.arr_elem_kind() != kind {
        return false;
    }
    let inner = chain >> 3;
    if kind != ARR_KIND_HEAP || (inner == 0 && !str_leaf) {
        return true;
    }
    unsafe { live_slots(arr) }.all(|p| {
        if p == 0 {
            return false;
        }
        let cell = p as *const u8;
        if inner != 0 {
            return unsafe { arr_cell_fits(cell, inner, str_leaf) };
        }
        unsafe { &*(cell as *const HeapHeader) }.tag() == Tag::Str
    })
}

/// The raw words of an array block's live slots, which start at the
/// deque head: a shifted array keeps its data pointer and moves the
/// head instead.
///
/// # Safety
/// `arr` is a live `Tag::Arr` block with raw (non-boxed) slots.
unsafe fn live_slots(arr: *const u8) -> impl Iterator<Item = u64> {
    let len = unsafe { *(arr.add(ARR_LEN_OFF) as *const u64) };
    let head = unsafe { *(arr.add(ARR_HDR_HEAD_OFF) as *const u32) } as u64;
    let data = unsafe { arr_data(arr) };
    (head..head + len).map(move |i| unsafe { *(data.add(i as usize * 8) as *const u64) })
}

/// # Safety
/// `arr` is a live heap cell.
unsafe fn arr_fits(arr: *const u8, class_tag: u32, depth: i64) -> bool {
    const UNFIT: u16 = FLAG_ARR_ANY | FLAG_ARR_EXOTIC_INDEX | FLAG_ARR_SPARSE_TAIL;
    let hdr = unsafe { &*(arr as *const HeapHeader) };
    if hdr.tag() != Tag::Arr || hdr.flags & UNFIT != 0 || hdr.arr_elem_kind() != ARR_KIND_HEAP {
        return false;
    }
    unsafe { live_slots(arr) }.all(|p| {
        if p == 0 {
            return false;
        }
        if depth <= 1 {
            let fits = unsafe { __torajs_anyv_struct_fits(p, class_tag) };
            return fits == 1;
        }
        unsafe { arr_fits(p as *const u8, class_tag, depth - 1) }
    })
}
