//! Own-key walk for the cells whose properties live only in a lazy
//! expando bag — split out of [`crate::obj_own_keys`] (file-size hard
//! limit; the parent keeps the dispatch, this file keeps the walk).

use core::ffi::c_void;

use crate::obj_own_keys::dynobj_keys_append;

unsafe extern "C" {
    fn __torajs_arr_alloc(cap: u64) -> *mut u8;
    fn __torajs_arr_push(arr: *mut u8, val: i64) -> *mut u8;
}

/// The own keys of a bag-only receiver, in §10.1.11.1 order.
///
/// Rotation 354 gave the promise cell this shape: no inherent own
/// key at all — `then` / `catch` are prototype surface — so the +32
/// expando the defineProperty / plain-assign arms write is the whole
/// answer. The for-in kernel rides the same call, so all four
/// enumeration spellings agree. Map / Set (§24.1.6 / §24.2.6) and
/// Date (§21.4.4) join it: their entry table and [[DateValue]] are
/// internal state, never properties.
///
/// RegExp leads with `lastIndex` (§22.2.4.1 RegExpAlloc makes it
/// {writable: true, enumerable: false, configurable: false}, so gOPN
/// reports it and `Object.keys` does not), then the same bag.
///
/// # Safety
/// `cell` is a live heap cell whose header tag is `htag`.
pub(crate) unsafe fn bag_cell_keys(
    cell: *const c_void,
    htag: u16,
    include_nonenum: i64,
) -> *mut c_void {
    let mut out = unsafe { __torajs_arr_alloc(0) };
    if htag == crate::obj_own_keys_layout::TAG_REGEXP_CELL && include_nonenum != 0 {
        // alloc_str_key mints rc=1 — the array slot adopts it.
        let k = unsafe { crate::reflect::alloc_str_key(b"lastIndex") };
        out = unsafe { __torajs_arr_push(out, k as i64) };
    }
    let props = unsafe { crate::obj_own_keys_layout::expando_props(cell, htag) };
    if props.is_null() {
        return out as *mut c_void;
    }
    unsafe { dynobj_keys_append(props, include_nonenum, out, false, false) as *mut c_void }
}
