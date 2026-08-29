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
use crate::obj_own_keys_proto_names::push_synthesized_proto_names;

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
    unsafe {
        dynobj_keys_append(props, include_nonenum, out as *mut u8, true, false) as *mut c_void
    }
}
