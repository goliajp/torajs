//! An arguments object's indices past its elements.
//!
//! §10.4.4 makes an arguments object an ordinary object with an
//! ordinary data `length`: `arguments[2] = 9` on a two-argument call
//! creates an own property "2" and leaves `length` at 2. The cell
//! behind `arguments` is an Arr, whose element store grows on such a
//! write and moves `length` with it — so the object then answered
//! `length` 3, `Array.prototype.every.call(arguments, cb)` visited
//! three elements, `indexOf.call(arguments, 9)` found index 2.
//!
//! Past-the-end index writes on an arguments cell therefore land in
//! the cell's expando props bag, the place its other own non-element
//! properties already live, and the element store keeps exactly the
//! argument list. Every generic algorithm that walks `0..length`
//! then sees the spec's length for free; the element readers consult
//! the bag when an index falls past the store. `in`, own-key
//! enumeration and gOPD already read the bag.

use core::ffi::c_void;

use torajs_rc::arr_flags_are_arguments;

unsafe extern "C" {
    fn __torajs_dynobj_has(dynobj: *const c_void, key: *const c_void) -> i32;
    fn __torajs_dynobj_entry_is_hole(dynobj: *const c_void, key: *const c_void) -> i32;
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
    fn __torajs_str_drop(s: *mut c_void);
}

/// Does index `i` of this cell live in the props bag rather than the
/// element store? Only for an arguments object, only past the store.
///
/// # Safety
/// `arr` is a live `Tag::Arr` heap pointer; `len` its element count.
#[inline]
pub(crate) unsafe fn in_bag(arr: *const c_void, i: u64, len: u64) -> bool {
    i >= len && arr_flags_are_arguments(unsafe { crate::define::header_flags(arr) })
}

/// The bag entry for index `i` as a BORROWED box (the bag keeps its
/// reference), or `None` when the arguments object has no such own
/// property.
///
/// # Safety
/// `arr` is a live `Tag::Arr` heap pointer.
pub(crate) unsafe fn bag_get_borrowed(arr: *const c_void, i: u64) -> Option<u64> {
    unsafe {
        let dynobj = *crate::define::props_slot(arr as *mut c_void);
        if dynobj.is_null() {
            return None;
        }
        let key = crate::define::mint_index_key(i) as *const c_void;
        let found = __torajs_dynobj_has(dynobj, key) != 0
            && __torajs_dynobj_entry_is_hole(dynobj, key) == 0;
        let boxed = found.then(|| {
            let tag = crate::props::__torajs_arrprops_get_tag(arr as *mut c_void, key);
            let value = crate::props::__torajs_arrprops_get_value(arr as *mut c_void, key);
            __torajs_anyv_box_from_pair(tag as i64, value as i64)
        });
        __torajs_str_drop(key as *mut c_void);
        boxed
    }
}

/// Store index `i` as an ordinary own data property in the bag. The
/// value's stake (`tag == 4`) transfers, as it does into a slot.
///
/// # Safety
/// `arr` is a live `Tag::Arr` heap pointer.
pub(crate) unsafe fn bag_store(arr: *mut c_void, i: u64, tag: u64, value: u64) {
    unsafe {
        let key = crate::define::mint_index_key(i);
        crate::props::__torajs_arrprops_set(arr, key as *const c_void, tag as i64, value as i64);
        __torajs_str_drop(key as *mut c_void);
    }
}
