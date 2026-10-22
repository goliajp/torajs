//! A typed array's read at a Number index that is not an integer.
//!
//! §7.1.19 names the property "1.5" (or "NaN"), never element 1. A
//! dense typed cell cannot hold such a property, so the read answers
//! the slot's undefined right here; only a cell that can carry a
//! named property — an own props bag, a subclass envelope, a minted
//! prototype on the Array chain — goes on to the keyed any-world
//! read, behind a link seam (default in `torajs-dispatch`) judged on
//! those states' writer entries.

use core::ffi::c_void;

use torajs_rc::builtin_proto::{
    __torajs_peek_builtin_prototype, ARRAY_PROTO_TAG, proto_parent_tag,
};
use torajs_rc::{F64_UNDEF_SENTINEL_BITS, FLAG_SUBCLASSED};

use crate::exotic_seam::{
    __torajs_arr_typed_nonint_any_slow, __torajs_arr_typed_nonint_f64_slow,
    __torajs_arr_typed_nonint_str_slow,
};
use crate::layout::{ARR_PROPS_OFF, VALUE_UNDEFINED_IMM};

unsafe extern "C" {
    fn __torajs_dynobj_has(dynobj: *const c_void, key: *const c_void) -> i32;
    fn __torajs_str_alloc_pooled(len: u64) -> *mut u8;
    fn __torajs_str_drop(s: *mut c_void);
}

/// Can the property `key` names (never an array index) be visible
/// through this cell: an own expando, a member of a subclass's
/// prototype, or a user property on %Array.prototype% /
/// %Object.prototype%? Each state has its writer entries —
/// `__torajs_arr_props_attach` and the regex exec attach,
/// `__torajs_arr_subclass_alloc`, `__torajs_get_builtin_prototype` —
/// and those are what the slow seams are link-judged on. An own bag
/// counts only when it holds the key: a bag of hole entries alone
/// (`__torajs_arr_props_attach_index`) carries no named property.
///
/// # Safety
/// `arr` is a live `Tag::Arr` cell.
unsafe fn named_prop_possible(arr: *const u8, key: f64) -> bool {
    unsafe {
        let props = *(arr.add(ARR_PROPS_OFF) as *const *const c_void);
        if (!props.is_null() && own_bag_has(props, key))
            || *(arr.add(6) as *const u16) & FLAG_SUBCLASSED != 0
        {
            return true;
        }
        let mut tag = ARRAY_PROTO_TAG as i64;
        while tag >= 0 {
            if !__torajs_peek_builtin_prototype(tag).is_null() {
                return true;
            }
            tag = proto_parent_tag(tag);
        }
        false
    }
}

/// §7.1.19 ToPropertyKey of a non-integral Number, looked up in the
/// own bag (the spelling `join` gives an element: ToString).
///
/// # Safety
/// `props` is a live dynobj.
unsafe fn own_bag_has(props: *const c_void, key: f64) -> bool {
    let mut buf = [0u8; 32];
    unsafe {
        let (digits, len) = crate::join::f64_to_string_piece(key, buf.as_mut_ptr(), buf.len());
        let s = __torajs_str_alloc_pooled(len);
        core::ptr::copy_nonoverlapping(digits, s.add(crate::define::STR_DATA_OFF), len as usize);
        let hit = __torajs_dynobj_has(props, s as *const c_void) != 0;
        __torajs_str_drop(s as *mut c_void);
        hit
    }
}

/// `number[]` read at a non-integral index: the F64 slot word —
/// the undefined sentinel when no such property can exist, the keyed
/// read's answer otherwise.
///
/// # Safety
/// `arr` is a live `Tag::Arr` cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_typed_nonint_f64(arr: *mut c_void, key: f64) -> f64 {
    unsafe {
        if !named_prop_possible(arr as *const u8, key) {
            return f64::from_bits(F64_UNDEF_SENTINEL_BITS);
        }
        __torajs_arr_typed_nonint_f64_slow(arr, key)
    }
}

/// `string[]` twin of [`__torajs_arr_typed_nonint_f64`]: an owned Str
/// cell, or NULL for the slot's undefined.
///
/// # Safety
/// `arr` is a live `Tag::Arr` cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_typed_nonint_str(arr: *mut c_void, key: f64) -> *mut c_void {
    unsafe {
        if !named_prop_possible(arr as *const u8, key) {
            return core::ptr::null_mut();
        }
        __torajs_arr_typed_nonint_str_slow(arr, key)
    }
}

/// The twin for an element with no Number or Str slot: a `boolean[]`
/// (`keep != 0`) answers the keyed read's whole value, owned, into
/// its promoted `any` slot; a pointer element (`keep == 0`) runs the
/// read for its getter's throw and answers `undefined`, its slot
/// having no spelling for a value of another type. A cell that
/// cannot carry the property answers `undefined` without the seam.
///
/// # Safety
/// `arr` is a live `Tag::Arr` cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_typed_nonint_any(
    arr: *mut c_void,
    key: f64,
    keep: i64,
) -> u64 {
    unsafe {
        if !named_prop_possible(arr as *const u8, key) {
            return VALUE_UNDEFINED_IMM;
        }
        __torajs_arr_typed_nonint_any_slow(arr, key, keep)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dense_cell_answers_undefined_without_the_seam() {
        // header + len + cap + props + data, zeroed: a dense cell
        let mut block = [0u64; 6];
        let p = block.as_mut_ptr() as *mut u8;
        unsafe {
            *(p as *mut u32) = 1;
            *(p.add(4) as *mut u16) = crate::layout::TAG_ARR;
            assert!(!named_prop_possible(p, 1.5));
            let got = __torajs_arr_typed_nonint_f64(p as *mut c_void, 1.5);
            assert_eq!(got.to_bits(), F64_UNDEF_SENTINEL_BITS);
            assert!(__torajs_arr_typed_nonint_str(p as *mut c_void, f64::NAN).is_null());
            let got = __torajs_arr_typed_nonint_any(p as *mut c_void, 5.0, 1);
            assert_eq!(got, VALUE_UNDEFINED_IMM);
        }
    }

    #[test]
    fn subclass_bit_takes_the_seam() {
        let mut block = [0u64; 6];
        let p = block.as_mut_ptr() as *mut u8;
        unsafe {
            *(p as *mut u32) = 1;
            *(p.add(4) as *mut u16) = crate::layout::TAG_ARR;
            *(p.add(6) as *mut u16) = FLAG_SUBCLASSED;
            assert!(named_prop_possible(p, f64::NAN));
        }
    }
}
