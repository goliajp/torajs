//! `delete arr[i]` hole semantics — RFC 20260713-defprop-tpd-cluster
//! chunk C, split out of `define.rs` (file-size hard limit).
//!
//! Element storage stays dense: a deleted canonical index drops its
//! element value (slot reads `undefined`) and gains a HOLE shadow
//! entry in the expando props dynobj (sentinel value slot — see
//! torajs-dynobj `set_entry_hole`). Every own-property consumer
//! treats the hole as absent through the `arr_index_flags` result's
//! [`crate::define::F_HOLE`] bit; a plain write or a fresh define
//! revives the index (the flags upsert clears the sentinel).

use core::ffi::c_void;

use torajs_rc::FLAG_ARR_EXOTIC_INDEX;

use crate::define::{
    ANY_UNDEF, F_CONFIGURABLE, F_HOLE, FLAGS_DEFAULT, STR_DATA_OFF, index_flags_with_key,
    props_slot, store_shadow,
};
use crate::layout::ARR_LEN_OFF;

unsafe extern "C" {
    fn __torajs_dynobj_alloc() -> *mut c_void;
    fn __torajs_dynobj_has(dynobj: *const c_void, key: *const c_void) -> i32;
    /// torajs-dynobj — HOLE sentinel upsert / probe (chunk C).
    fn __torajs_dynobj_set_entry_hole(obj_slot: *mut *mut c_void, key: *mut c_void);
    fn __torajs_dynobj_entry_is_hole(dynobj: *const c_void, key: *const c_void) -> i32;
    fn __torajs_str_alloc_pooled(len: u64) -> *mut u8;
    fn __torajs_str_drop(s: *mut c_void);
}

/// Re-create a deleted index as a default data property — the plain
/// `arr[i] = v` write path calls this after the element store when
/// the index was a hole (§10.1.5.1 CreateDataProperty completes to
/// w/e/c all true, and the flags upsert clears the hole sentinel).
///
/// # Safety
/// `arr` is a live `Tag::Arr` heap pointer; `key` a live Str.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_index_revive(arr: *mut c_void, key: *mut c_void) {
    unsafe { store_shadow(arr, key, FLAGS_DEFAULT) };
}

/// Numeric-index twin of [`__torajs_arr_index_revive`] for the
/// any-tier element write lanes (`arr[i] = v` with a number index
/// never mints a key Str) — mint, revive iff the index is currently
/// a hole, drop. Callers gate on `FLAG_ARR_EXOTIC_INDEX` so the
/// plain-array hot path never reaches here.
pub(crate) unsafe fn revive_index_if_hole(arr: *mut c_void, idx: u64) {
    let mut buf = [0u8; 20];
    let mut n = buf.len();
    let mut v = idx;
    loop {
        n -= 1;
        buf[n] = b'0' + (v % 10) as u8;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    let digits = &buf[n..];
    let key = unsafe { __torajs_str_alloc_pooled(digits.len() as u64) };
    unsafe { core::ptr::copy_nonoverlapping(digits.as_ptr(), key.add(STR_DATA_OFF), digits.len()) };
    let props = unsafe { *props_slot(arr) };
    if !props.is_null()
        && unsafe { __torajs_dynobj_has(props, key as *const c_void) } != 0
        && unsafe { __torajs_dynobj_entry_is_hole(props, key as *const c_void) } != 0
    {
        unsafe { store_shadow(arr, key as *mut c_void, FLAGS_DEFAULT) };
    }
    unsafe { __torajs_str_drop(key as *mut c_void) };
}

/// Mark every index in `[from, to)` a HOLE — the §10.4.2.5
/// length-grow tail (RFC 20260721 刀 5 G3: grown slots are not own
/// properties). Same shadow-entry representation as `delete`, so
/// every existing consumer (has / gOPD / enumeration / reads)
/// answers absent for free.
pub(crate) unsafe fn mark_hole_range(arr: *mut c_void, from: u64, to: u64) {
    if from >= to {
        return;
    }
    unsafe {
        let slot = props_slot(arr);
        if (*slot).is_null() {
            *slot = __torajs_dynobj_alloc();
        }
        for i in from..to {
            let key = crate::define::mint_index_key(i);
            __torajs_dynobj_set_entry_hole(slot, key as *mut c_void);
            __torajs_str_drop(key as *mut c_void);
        }
        let p = (arr as *mut u8).add(6) as *mut u16;
        p.write(p.read() | FLAG_ARR_EXOTIC_INDEX);
    }
}

/// Mark the LAST pushed slot of a freshly-built array literal a
/// HOLE (§13.2.4 elision — the slot reads undefined but is not an
/// own property: `1 in [0,,2]` is false, indexOf skips it). Called
/// by the array-literal lowering right after the elision slot's
/// push, so `len - 1` is the elision index; the fresh slot carries
/// default attributes, so the delete below cannot refuse.
///
/// # Safety
/// `arr` is a live `Tag::Arr` heap pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_mark_last_hole(arr: *mut c_void) {
    let len = unsafe { (arr.cast::<u8>().add(ARR_LEN_OFF) as *const u64).read() };
    if len == 0 {
        return;
    }
    let idx = len - 1;
    let key = unsafe { crate::define::mint_index_key(idx) };
    unsafe {
        __torajs_arr_delete_index(arr, key as *mut c_void, idx);
        __torajs_str_drop(key as *mut c_void);
    }
}

/// §10.4.2 [[Delete]] on a canonical index — `delete arr[i]`.
/// Answers 1 (deleted / already absent) or 0 (refused: the index is
/// non-configurable — caller throws per §13.5.1.2 strict semantics).
///
/// # Safety
/// `arr` is a live `Tag::Arr` heap pointer; `key` a live Str whose
/// bytes already parsed as canonical index `idx`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_delete_index(
    arr: *mut c_void,
    key: *mut c_void,
    idx: u64,
) -> i32 {
    let len = unsafe { (arr.cast::<u8>().add(ARR_LEN_OFF) as *const u64).read() };
    if idx >= len {
        return 1;
    }
    let cur_flags = unsafe { index_flags_with_key(arr, key as *const c_void) };
    if cur_flags & F_HOLE != 0 {
        return 1;
    }
    if cur_flags & F_CONFIGURABLE == 0 {
        return 0;
    }
    // Drop the element's value (kind-aware store of undefined keeps
    // the dense model; reads through the hole answer undefined
    // regardless).
    unsafe { crate::index_any::__torajs_arr_index_set(arr, idx as i64, ANY_UNDEF, 0) };
    let slot = unsafe { props_slot(arr) };
    if unsafe { (*slot).is_null() } {
        unsafe { *slot = __torajs_dynobj_alloc() };
    }
    unsafe { __torajs_dynobj_set_entry_hole(slot, key) };
    let p = unsafe { (arr as *mut u8).add(6) as *mut u16 };
    unsafe { p.write(p.read() | FLAG_ARR_EXOTIC_INDEX) };
    1
}
