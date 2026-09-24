//! The Arr-cell leg of the own-key walk — split out of
//! `obj_own_keys.rs`, which the `Function.prototype` closure leg
//! pushed past the 500-line limit.
//!
//! §23.1.3 makes `Array.prototype` an Array exotic object rather than
//! an ordinary one, so an Arr cell can be a builtin prototype and its
//! own keys are two lists joined: the index domain the element
//! storage owns, and the expando entries every other shape keeps in a
//! dynobj. The parent's dispatch calls in here for both.

use core::ffi::c_void;

use crate::obj_own_keys::dynobj_keys_append;
use crate::obj_own_keys_key_shape::key_canonical_index;
use crate::obj_own_keys_layout::FLAG_ENUMERABLE;
use crate::obj_own_keys_proto_names::push_synthesized_proto_names;

unsafe extern "C" {
    fn __torajs_arr_push(arr: *mut u8, val: i64) -> *mut u8;
    fn __torajs_rc_inc(p: *mut c_void);
    /// torajs-dynobj iteration surface — keys are BORROWED.
    fn __torajs_dynobj_iter_len(obj: *const c_void) -> u64;
    fn __torajs_dynobj_iter_key(obj: *const c_void, i: u64) -> *mut c_void;
    fn __torajs_dynobj_iter_order(obj: *const c_void, out: *mut u64, cap: u64) -> u64;
    fn __torajs_dynobj_iter_flags(obj: *const c_void, i: u64) -> u64;
    fn __torajs_dynobj_entry_is_hole(dynobj: *const c_void, key: *const c_void) -> i32;
    /// torajs-arr — 1 when the cell is an arguments object.
    fn __torajs_arr_is_arguments(arr: *const c_void) -> i64;
}

/// `torajs-arr` layout mirror — the element count at +8.
const ARR_LEN_OFF: usize = 8;

/// `torajs-arr` layout mirror — the expando dynobj slot at +24.
const ARR_PROPS_OFF: usize = 24;

/// Index-key list `["0", ..., "<len-1>"]`, plus a trailing
/// `"length"` on the gOPN surface (`include_nonenum = 1`) — shared by
/// the Str and Arr ToObject arms.
pub(crate) unsafe fn index_keys(len: i64, include_nonenum: i64) -> *mut c_void {
    if include_nonenum == 0 {
        unsafe { crate::own_names::__torajs_arr_keys_only(len) }
    } else {
        unsafe { crate::own_names::__torajs_arr_index_strs(len) }
    }
}

/// Arr-cell own keys: index keys (+ `"length"` for gOPN, §10.4.2)
/// followed by expando keys from the inline props dynobj (insertion
/// order — `length` predates any expando write, matching the ES
/// OrdinaryOwnPropertyKeys creation-order tail).
pub(crate) unsafe fn arr_cell_keys(cell: *const c_void, include_nonenum: i64) -> *mut c_void {
    // RFC 20260712-arr-exotic-define chunk C — both surfaces ride
    // exotic-aware helpers: keys filters per-index enumerable, gOPN
    // keeps non-enumerable indices but skips deleted (hole) ones
    // (RFC 20260713 chunk C).
    let mut out = if include_nonenum == 0 {
        unsafe { crate::own_names::__torajs_arr_keys_only_of(cell) }
    } else {
        unsafe { crate::own_names::__torajs_arr_index_strs_of(cell) }
    };
    // §23.1.3 makes `Array.prototype` an Arr cell rather than a
    // dynobj, so its synthesized method names come through here
    // instead of the walk above — same surface, other cell shape.
    if include_nonenum != 0 {
        out = unsafe { push_synthesized_proto_names(cell, out as *mut u8, true) as *mut c_void };
    }
    let props =
        unsafe { (cell.cast::<u8>().add(ARR_PROPS_OFF) as *const u64).read() } as *const c_void;
    if props.is_null() {
        return out;
    }
    if unsafe { __torajs_arr_is_arguments(cell) } != 0 {
        out = unsafe { push_arguments_bag_indices(cell, props, include_nonenum, out as *mut u8) }
            as *mut c_void;
    }
    unsafe {
        dynobj_keys_append(props, include_nonenum, out as *mut u8, true, false) as *mut c_void
    }
}

/// §10.4.4 — an arguments object's index properties past its elements
/// live in the props bag (`torajs-arr::arguments_index_bag`). They are
/// integer keys, so §10.1.11.1 lists them after the element indices in
/// ascending order and before every string key; the bag walk that
/// follows skips every canonical index, as it does for an Array.
unsafe fn push_arguments_bag_indices(
    cell: *const c_void,
    props: *const c_void,
    include_nonenum: i64,
    mut out: *mut u8,
) -> *mut u8 {
    let len = unsafe { (cell.cast::<u8>().add(ARR_LEN_OFF) as *const u64).read() };
    let n = unsafe { __torajs_dynobj_iter_len(props) };
    let mut order = vec![0u64; n as usize];
    let n = unsafe { __torajs_dynobj_iter_order(props, order.as_mut_ptr(), n) };
    let mut found: Vec<(u64, *mut c_void)> = Vec::new();
    for &i in order.iter().take(n as usize) {
        let key = unsafe { __torajs_dynobj_iter_key(props, i) };
        if key.is_null() {
            continue;
        }
        let Some(idx) = (unsafe { key_canonical_index(key) }) else {
            continue;
        };
        if idx < len
            || unsafe { __torajs_dynobj_entry_is_hole(props, key) } != 0
            || (include_nonenum == 0
                && unsafe { __torajs_dynobj_iter_flags(props, i) } & FLAG_ENUMERABLE == 0)
        {
            continue;
        }
        found.push((idx, key));
    }
    found.sort_unstable_by_key(|&(idx, _)| idx);
    for (_, key) in found {
        // Borrowed key → the array slot takes its own share.
        unsafe { __torajs_rc_inc(key) };
        out = unsafe { __torajs_arr_push(out, key as i64) };
    }
    out
}
