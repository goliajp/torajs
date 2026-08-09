//! The materialized `arguments` object's `"length"` face — §10.4.4
//! gives an arguments exotic object a PLAIN data `length`
//! ({writable: true, enumerable: false, **configurable: true**}),
//! where an Array's length is the §10.4.2 non-configurable exotic
//! slot. Both desugar lanes mint the `__torajs_arguments` local as a
//! `Tag::Arr` cell, so the distinction lives in
//! [`FLAG_ARR_ARGUMENTS`], stamped by the lowering right after the
//! mint; the keyed readers (gOPD / delete / hasOwnProperty /
//! member-get) gate on it.
//!
//! A delete leaves a HOLE shadow entry under the `"length"` key in
//! the expando props dynobj — the element-domain tombstone
//! representation (RFC 20260712 chunk C), so every enumerator and
//! bag reader already treats it as absent. A later
//! `defineProperty(args, "length", …)` restore is a recorded face
//! (L3b) — the t262 verifyProperty probe deletes without the restore
//! option.

use core::ffi::c_void;

use torajs_rc::FLAG_ARR_ARGUMENTS;

use crate::define::{header_flags, props_slot};

unsafe extern "C" {
    fn __torajs_dynobj_alloc() -> *mut c_void;
    fn __torajs_dynobj_has(dynobj: *const c_void, key: *const c_void) -> i32;
    /// torajs-dynobj — HOLE sentinel upsert / probe (chunk C). The
    /// upsert is a no-op on a NULL bag — the delete below allocates
    /// the lazy expando first (the `store_shadow` idiom).
    fn __torajs_dynobj_set_entry_hole(obj_slot: *mut *mut c_void, key: *mut c_void);
    fn __torajs_dynobj_entry_is_hole(dynobj: *const c_void, key: *const c_void) -> i32;
}

/// Stamp the freshly minted `__torajs_arguments` cell — called once
/// per materialization by both lowering lanes (the argv-form
/// expansion and the literal-form mark statement). Idempotent.
///
/// # Safety
/// `arr` is a live `Tag::Arr` heap pointer (or NULL, a no-op).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_mark_arguments(arr: *mut c_void) {
    if arr.is_null() {
        return;
    }
    let fp = unsafe { (arr as *mut u8).add(6) }.cast::<u16>();
    unsafe { fp.write(fp.read() | FLAG_ARR_ARGUMENTS) };
}

/// The `"length"` face state of an Arr cell: 0 = plain array (the
/// §10.4.2 non-configurable length), 1 = arguments materialization
/// with its length intact, 2 = arguments materialization whose
/// length was deleted (the hole tombstone is present).
///
/// # Safety
/// `arr` is a live `Tag::Arr` heap pointer; `key` is the caller's
/// live `"length"` Str cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_arguments_length_state(
    arr: *const c_void,
    key: *const c_void,
) -> i64 {
    unsafe {
        if header_flags(arr) & FLAG_ARR_ARGUMENTS == 0 {
            return 0;
        }
        let props = *props_slot(arr as *mut c_void);
        if !props.is_null()
            && __torajs_dynobj_has(props, key) != 0
            && __torajs_dynobj_entry_is_hole(props, key) != 0
        {
            return 2;
        }
        1
    }
}

/// `delete args.length` — a configurable data property deletes
/// (§10.4.4 has no length special-case), leaving the hole tombstone.
/// Answers 1 when the cell is an arguments materialization; 0 for a
/// plain array (the caller keeps the §10.4.2 refusal).
///
/// # Safety
/// `arr` is a live `Tag::Arr` heap pointer; `key` is a live
/// `"length"` Str cell (borrowed — the hole upsert takes its own
/// stake through the dynobj entry mint).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_arguments_length_delete(
    arr: *mut c_void,
    key: *mut c_void,
) -> i64 {
    unsafe {
        if header_flags(arr as *const c_void) & FLAG_ARR_ARGUMENTS == 0 {
            return 0;
        }
        let slot = props_slot(arr);
        if (*slot).is_null() {
            *slot = __torajs_dynobj_alloc();
        }
        __torajs_dynobj_set_entry_hole(slot, key);
        1
    }
}
