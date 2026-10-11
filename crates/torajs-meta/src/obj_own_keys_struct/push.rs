//! The values and entries walks' pushers over the expando entries a
//! lane selects. Moved out of [`super`] whole.

use core::ffi::c_void;

use super::{
    __torajs_arr_alloc_any, __torajs_arr_push, __torajs_arr_push_any, __torajs_rc_inc, ANY_HEAP,
    ExpandoLane, for_each_enumerable_expando,
};

/// Append the expando VALUES `lane` selects to an `Arr<Any>` (the
/// values walk's pusher; the array takes a fresh share of a heap
/// value).
///
/// # Safety
/// `cell` is a live `Tag::Obj` cell; `out` an owned `Arr<Any>`.
pub(crate) unsafe fn push_expando_values(
    cell: *const c_void,
    lane: ExpandoLane,
    out: &mut *mut u8,
) {
    unsafe {
        for_each_enumerable_expando(cell, lane, |_key, tag, val| {
            if tag == ANY_HEAP {
                __torajs_rc_inc(val as *mut c_void);
            }
            *out = __torajs_arr_push_any((*out).cast::<c_void>(), tag, val);
        })
    }
}

/// Append the expando `[key, value]` PAIRS `lane` selects to an
/// `Arr<Arr<Any>>` (the entries walk's pusher; both reads borrow, the
/// inner array takes fresh shares).
///
/// # Safety
/// `cell` is a live `Tag::Obj` cell; `outer` an owned `Arr<Arr<Any>>`.
pub(crate) unsafe fn push_expando_entries(
    cell: *const c_void,
    lane: ExpandoLane,
    outer: &mut *mut u8,
) {
    unsafe {
        for_each_enumerable_expando(cell, lane, |key, tag, val| {
            __torajs_rc_inc(key);
            if tag == ANY_HEAP {
                __torajs_rc_inc(val as *mut c_void);
            }
            let inner = __torajs_arr_alloc_any(2);
            let inner = __torajs_arr_push_any(inner.cast::<c_void>(), ANY_HEAP, key as u64);
            let inner = __torajs_arr_push_any(inner.cast::<c_void>(), tag, val);
            *outer = __torajs_arr_push(*outer, inner as i64);
        })
    }
}
