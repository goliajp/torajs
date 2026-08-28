//! `any`-receiver Array early-exit predicate walks (any-dispatch
//! backfill chunk 3) — `a.every(cb)` / `some` / `find` / `findIndex`
//! / `findLast` / `findLastIndex` where `a` crossed into the `any`
//! world.
//!
//! Split out of [`crate::method_any_hof`], which keeps the walks
//! that visit every index and build a product (map / filter /
//! forEach / reduce). The seam is what each shape answers: these
//! stop at the first element that decides the outcome and never
//! allocate a result cell, so they carry no destination ledger and
//! no hole gate. Callback ABI, argv layout and the value ledger are
//! that module's — `undef` / `ARGV_SLOTS` / `BoxedFn` /
//! `recv_first_shift` are shared verbatim rather than mirrored.

use core::ffi::c_void;

use crate::layout::ARR_LEN_OFF;
use crate::method_any_hof::{ARGV_SLOTS, BoxedFn, recv_first_shift, undef};

unsafe extern "C" {
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
    /// torajs-anyvalue — ES ToBoolean over an AnyValue (the
    /// predicate coercion).
    fn __torajs_anyv_to_bool(v: u64) -> bool;
    /// Cross-tier — universal NaN-box-safe heap dropper.
    fn __torajs_value_drop_heap(p: *mut c_void);
    /// Cross-tier — torajs-throw. Non-zero iff a throw is pending.
    fn __torajs_throw_check() -> i64;
}

/// Shared early-exit predicate loop (any-dispatch backfill chunk 3).
/// `mode`: 0 = every, 1 = some, 2 = find, 3 = findIndex,
/// 4 = findLast, 5 = findLastIndex (modes ≥ 4 walk backwards,
/// §23.1.3.11 / §23.1.3.12). Same ledger as `method_any_hof`'s `hof_loop`; on
/// the exit hit `find` / `findLast` transfer the owned element
/// read as the return, the others release it.
unsafe fn find_loop(
    arr: *const c_void,
    cb_env: *mut c_void,
    cb_entry: u64,
    mode: i64,
    this_arg: u64,
) -> u64 {
    unsafe {
        // RFC 20260810 刀 D — same loud reject as `hof_loop`.
        if crate::sparse_gate::sparse_tail_rejects(
            arr,
            b"sparse array tail is not yet supported in the Array.prototype find/every/some family\0"
                .as_ptr(),
        ) {
            return undef();
        }
        let cb: BoxedFn = core::mem::transmute(cb_entry as usize);
        let s = recv_first_shift(cb_env);
        let len = *((arr as *const u8).add(ARR_LEN_OFF) as *const u64);
        let arr_boxed = arr as u64;
        let right = mode >= 4;
        let mut step: u64 = 0;
        while step < len {
            let i = if right { len - 1 - step } else { step };
            let v = crate::index_any::__torajs_arr_index_get(arr, i as i64);
            let mut argv = [undef(); ARGV_SLOTS];
            if s == 1 {
                // knife 4 — the thisArg (or undefined) rides argv[0]
                // for a receiver-first callback.
                argv[0] = this_arg;
            }
            argv[s] = v;
            argv[s + 1] = __torajs_anyv_box_from_pair(2, i as i64);
            argv[s + 2] = arr_boxed;
            let r = cb(cb_env, argv.as_ptr(), (3 + s) as i64);
            if __torajs_throw_check() != 0 {
                __torajs_value_drop_heap(v as *mut c_void);
                __torajs_value_drop_heap(r as *mut c_void);
                return undef();
            }
            let hit = __torajs_anyv_to_bool(r);
            __torajs_value_drop_heap(r as *mut c_void);
            match mode {
                // every — the first false predicate answers false.
                0 => {
                    __torajs_value_drop_heap(v as *mut c_void);
                    if !hit {
                        return __torajs_anyv_box_from_pair(1, 0);
                    }
                }
                // some — the first true predicate answers true.
                1 => {
                    __torajs_value_drop_heap(v as *mut c_void);
                    if hit {
                        return __torajs_anyv_box_from_pair(1, 1);
                    }
                }
                // find / findLast — the hit transfers the owned
                // element read.
                2 | 4 => {
                    if hit {
                        return v;
                    }
                    __torajs_value_drop_heap(v as *mut c_void);
                }
                // findIndex / findLastIndex — the hit answers the
                // index.
                _ => {
                    __torajs_value_drop_heap(v as *mut c_void);
                    if hit {
                        return __torajs_anyv_box_from_pair(2, i as i64);
                    }
                }
            }
            step += 1;
        }
        match mode {
            0 => __torajs_anyv_box_from_pair(1, 1),
            1 => __torajs_anyv_box_from_pair(1, 0),
            2 | 4 => undef(),
            _ => __torajs_anyv_box_from_pair(2, -1),
        }
    }
}

/// `a.every(cb)` per ES §23.1.3.6.
///
/// # Safety
/// See [`crate::method_any_hof::__torajs_arr_any_map`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_any_every(
    arr: *const c_void,
    cb_env: *mut c_void,
    cb_entry: u64,
    this_arg: u64,
) -> u64 {
    unsafe { find_loop(arr, cb_env, cb_entry, 0, this_arg) }
}

/// `a.some(cb)` per ES §23.1.3.29.
///
/// # Safety
/// See [`crate::method_any_hof::__torajs_arr_any_map`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_any_some(
    arr: *const c_void,
    cb_env: *mut c_void,
    cb_entry: u64,
    this_arg: u64,
) -> u64 {
    unsafe { find_loop(arr, cb_env, cb_entry, 1, this_arg) }
}

/// `a.find(cb)` per ES §23.1.3.9 — the matching element (owned) or
/// `undefined`.
///
/// # Safety
/// See [`crate::method_any_hof::__torajs_arr_any_map`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_any_find(
    arr: *const c_void,
    cb_env: *mut c_void,
    cb_entry: u64,
    this_arg: u64,
) -> u64 {
    unsafe { find_loop(arr, cb_env, cb_entry, 2, this_arg) }
}

/// `a.findIndex(cb)` per ES §23.1.3.10 — the matching index or -1.
///
/// # Safety
/// See [`crate::method_any_hof::__torajs_arr_any_map`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_any_find_index(
    arr: *const c_void,
    cb_env: *mut c_void,
    cb_entry: u64,
    this_arg: u64,
) -> u64 {
    unsafe { find_loop(arr, cb_env, cb_entry, 3, this_arg) }
}

/// `a.findLast(cb)` per ES §23.1.3.11 — backwards walk, the
/// matching element (owned) or `undefined`.
///
/// # Safety
/// See [`crate::method_any_hof::__torajs_arr_any_map`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_any_find_last(
    arr: *const c_void,
    cb_env: *mut c_void,
    cb_entry: u64,
    this_arg: u64,
) -> u64 {
    unsafe { find_loop(arr, cb_env, cb_entry, 4, this_arg) }
}

/// `a.findLastIndex(cb)` per ES §23.1.3.12 — backwards walk, the
/// matching index or -1.
///
/// # Safety
/// See [`crate::method_any_hof::__torajs_arr_any_map`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_any_find_last_index(
    arr: *const c_void,
    cb_env: *mut c_void,
    cb_entry: u64,
    this_arg: u64,
) -> u64 {
    unsafe { find_loop(arr, cb_env, cb_entry, 5, this_arg) }
}
