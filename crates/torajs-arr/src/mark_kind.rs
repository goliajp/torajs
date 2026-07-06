//! `__torajs_arr_mark_kind` — write the element-kind field on a typed
//! `Array<T>` heap header at the typed→Any boxing boundary.
//!
//! Any-dynamic-access RFC (20260704) S1. ssa-lower's
//! `box_to_tag_value` / `box_to_any` emit one call per
//! `Type::Arr(..) → Any` coercion, passing a **kind chain**: 3 bits
//! per nesting level, little-endian (level 0 = the array being
//! boxed). `Array<Array<i64>>` boxes with
//! `chain = ARR_KIND_HEAP | (ARR_KIND_I64 << 3)`; the HEAP level
//! walks its pointer slots and recurses into nested `Tag::Arr` cells
//! with the remaining chain, so inner typed arrays become
//! self-describing too. Boxing is a slow path — the O(len) walk only
//! runs when a nested-array chain is present.
//!
//! Gates:
//! - NULL → no-op
//! - `FLAG_STATIC_LITERAL` → no-op (never write `.rodata`; no baked
//!   array literals exist today, but the boundary is real)
//! - `FLAG_ARR_ANY` → no-op (NaN-box slots are self-describing;
//!   nested typed arrays inside Any slots are marked at their own
//!   boxing sites in `lower_array_any_literal`'s `box_to_tag_value`)
//! - already-marked with the same kind and no deeper chain → early
//!   return (repeated boxing of the same binding is idempotent)

use core::ffi::c_void;

use torajs_rc::{
    ARR_ELEM_KIND_MASK, ARR_ELEM_KIND_SHIFT, ARR_KIND_HEAP, FLAG_ARR_ANY, FLAG_STATIC_LITERAL,
    HeapHeader, Tag,
};

use crate::layout::{ARR_LEN_OFF, arr_data};

/// Write `chain & 7` into the header's element-kind field; when that
/// level is [`ARR_KIND_HEAP`] and a deeper chain remains, walk the
/// pointer slots and recurse into nested `Tag::Arr` cells.
///
/// # Safety
/// `arr` is either NULL or a valid `Tag::Arr` heap pointer. Chain
/// levels must describe the array's true element representation
/// (ssa-lower derives them from the static `Type::Arr` element chain).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_mark_kind(arr: *mut c_void, chain: u64) {
    if arr.is_null() {
        return;
    }
    let header = unsafe { &mut *(arr as *mut HeapHeader) };
    if header.flags & (FLAG_STATIC_LITERAL | FLAG_ARR_ANY) != 0 {
        return;
    }
    let kind = (chain & 0b111) as u16;
    let rest = chain >> 3;
    if header.arr_elem_kind() == kind && rest == 0 {
        return;
    }
    header.flags = (header.flags & !ARR_ELEM_KIND_MASK) | (kind << ARR_ELEM_KIND_SHIFT);
    if kind != ARR_KIND_HEAP || rest == 0 {
        return;
    }
    // Nested-array chain — each slot is a heap cell pointer; recurse
    // into the Tag::Arr ones with the remaining chain (Str / Obj /
    // other heap elements are self-describing, skip).
    unsafe {
        let arr_u8 = arr as *mut u8;
        let len = *(arr_u8.add(ARR_LEN_OFF) as *const u64);
        for i in 0..len {
            let slot = *(arr_data(arr_u8).add((i as usize) * 8) as *const u64);
            let child = slot as *mut c_void;
            if child.is_null() {
                continue;
            }
            let child_tag = (child.cast::<u8>().add(4) as *const u16).read();
            if child_tag == Tag::Arr as u16 {
                __torajs_arr_mark_kind(child, rest);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_rc::{ARR_KIND_I64, ARR_KIND_UNSET};

    #[test]
    fn mark_null_is_noop() {
        unsafe { __torajs_arr_mark_kind(core::ptr::null_mut(), ARR_KIND_I64 as u64) };
    }

    #[test]
    fn mark_writes_kind_and_preserves_other_flags() {
        // Hand-build a minimal Tag::Arr header block (no slots needed
        // for a scalar kind).
        let mut block = [0u8; 32];
        let p = block.as_mut_ptr();
        unsafe {
            *(p as *mut u32) = 1;
            *(p.add(4) as *mut u16) = crate::layout::TAG_ARR;
            *(p.add(6) as *mut u16) = torajs_rc::FLAG_FROZEN;
            *(p.add(ARR_LEN_OFF) as *mut u64) = 0;
            __torajs_arr_mark_kind(p as *mut c_void, ARR_KIND_I64 as u64);
            let header = &*(p as *const HeapHeader);
            assert_eq!(header.arr_elem_kind(), ARR_KIND_I64);
            assert_ne!(header.flags & torajs_rc::FLAG_FROZEN, 0);
        }
    }

    #[test]
    fn mark_static_literal_is_noop() {
        let mut block = [0u8; 32];
        let p = block.as_mut_ptr();
        unsafe {
            *(p as *mut u32) = 1;
            *(p.add(4) as *mut u16) = crate::layout::TAG_ARR;
            *(p.add(6) as *mut u16) = FLAG_STATIC_LITERAL;
            __torajs_arr_mark_kind(p as *mut c_void, ARR_KIND_I64 as u64);
            let header = &*(p as *const HeapHeader);
            assert_eq!(header.arr_elem_kind(), ARR_KIND_UNSET);
        }
    }

    #[test]
    fn mark_heap_chain_recurses_into_nested_arr() {
        // outer = Arr with one slot pointing at inner Arr; chain =
        // HEAP | (I64 << 3) must set outer=HEAP, inner=I64.
        let mut inner = [0u8; 40];
        let ip = inner.as_mut_ptr();
        let mut outer = [0u8; 48];
        let op = outer.as_mut_ptr();
        unsafe {
            *(ip as *mut u32) = 1;
            *(ip.add(4) as *mut u16) = crate::layout::TAG_ARR;
            *(ip.add(6) as *mut u16) = 0;
            *(ip.add(ARR_LEN_OFF) as *mut u64) = 0;
            *(ip.add(crate::layout::ARR_DATA_PTR_OFF) as *mut *mut u8) =
                ip.add(crate::layout::ARR_CELL_SIZE);

            *(op as *mut u32) = 1;
            *(op.add(4) as *mut u16) = crate::layout::TAG_ARR;
            *(op.add(6) as *mut u16) = 0;
            *(op.add(ARR_LEN_OFF) as *mut u64) = 1;
            *(op.add(crate::layout::ARR_DATA_PTR_OFF) as *mut *mut u8) =
                op.add(crate::layout::ARR_CELL_SIZE);
            *(arr_data(op) as *mut u64) = ip as u64;

            let chain = (ARR_KIND_HEAP as u64) | ((ARR_KIND_I64 as u64) << 3);
            __torajs_arr_mark_kind(op as *mut c_void, chain);
            assert_eq!((&*(op as *const HeapHeader)).arr_elem_kind(), ARR_KIND_HEAP);
            assert_eq!((&*(ip as *const HeapHeader)).arr_elem_kind(), ARR_KIND_I64);
        }
    }
}
