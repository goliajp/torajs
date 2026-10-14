//! AddEntriesFromIterable (ECMA-262 §24.1.1.2, the step §20.1.2.7
//! takes) over an iterable that is not one of the storage shapes the
//! parent walks directly: a user iterator object, a generator, a
//! helper. The iteration protocol is the runtime's own
//! (`__torajs_any_iter_next`), so `next` / `done` / `value` are read
//! the way a `for (const e of x)` reads them.
//!
//! The completions follow the algorithm: a throwing `next()` ends the
//! walk without closing the iterator (step 4.a), a non-object entry is
//! a TypeError that closes it (4.d), and an abrupt `Get(entry, "0")` /
//! `Get(entry, "1")` closes it with that throw winning (4.f / 4.h).

use core::ffi::c_void;

use super::{
    arr_cell, dynobj_cell, set_prop_from_dynobj_entry, set_prop_from_pair_arr,
    set_prop_from_struct_entry, struct_cell,
};
use crate::reflect::VALUE_UNDEFINED_IMM;

unsafe extern "C" {
    fn __torajs_any_iter_next(recv: u64, idx: *mut i64, iter: *mut u64, out: *mut u64) -> i64;
    fn __torajs_iter_close_abrupt(iter: u64);
    fn __torajs_throw_check() -> i64;
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    fn __torajs_anyv_rc_dec(v: u64);
    fn __torajs_dynobj_alloc() -> *mut c_void;
    fn __torajs_value_drop_heap(p: *mut c_void);
}

/// The fresh object built from `entries`, or `undefined` with a throw
/// pending.
///
/// # Safety
/// `entries` is a live cell-encoded AnyValue; `k0` / `k1` are the
/// live "0" / "1" key cells, which stay the caller's.
pub(super) unsafe fn from_iterable(entries: u64, k0: *const u8, k1: *const u8) -> u64 {
    unsafe {
        let mut obj = __torajs_dynobj_alloc();
        let (mut idx, mut iter, mut entry) = (0i64, VALUE_UNDEFINED_IMM, VALUE_UNDEFINED_IMM);
        let ok = loop {
            let more = __torajs_any_iter_next(entries, &mut idx, &mut iter, &mut entry);
            if __torajs_throw_check() != 0 {
                break false;
            }
            if more == 0 {
                break true;
            }
            if arr_cell(entry).is_some() {
                set_prop_from_pair_arr(&mut obj, entry as *const c_void);
            } else if dynobj_cell(entry).is_some() {
                set_prop_from_dynobj_entry(&mut obj, entry as *const c_void, k0, k1);
            } else if struct_cell(entry).is_some() {
                set_prop_from_struct_entry(&mut obj, entry, k0, k1);
            } else {
                __torajs_throw_type_error(c"Object.fromEntries entry is not an object".as_ptr());
            }
            __torajs_anyv_rc_dec(entry);
            entry = VALUE_UNDEFINED_IMM;
            if __torajs_throw_check() != 0 {
                __torajs_iter_close_abrupt(iter);
                break false;
            }
        };
        __torajs_anyv_rc_dec(iter);
        if ok {
            return obj as u64;
        }
        __torajs_value_drop_heap(obj);
        VALUE_UNDEFINED_IMM
    }
}
