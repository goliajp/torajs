//! `any`-receiver Array higher-order methods (Any-method-call RFC
//! 20260704 C3b) — `a.map(cb)` / `a.filter(cb)` / `a.forEach(cb)`
//! where `a` crossed into the `any` world.
//!
//! The typed tier unrolls these loops inline at compile time
//! (`ssa_lower_call_arr_ho`) with a statically-known callback; here
//! neither the element repr nor the callback signature is static, so
//! the loop runs in native code over the S3-get kernel
//! ([`crate::index_any::__torajs_arr_index_get`], kind-aware boxed
//! read) and invokes the callback through its boxed dual entry
//! (`(env, argv, argc) -> AnyValue` — the C3a adapters). The
//! dispatcher (torajs-anyvalue) has already verified the callback is
//! a closure cell with a non-zero entry and hands the `(env, entry)`
//! pair down.
//!
//! Per ES §23.1.3.19/8/15 the callback receives `(value, index,
//! array)`; the argv buffer pads to the adapters' fixed 8-slot
//! width with `undefined`.
//!
//! Ledger: argv slots are BORROWED by the callee (adapters unbox
//! borrows; an Any-param body that stores one incs at its own store
//! site). The element read is +1-owned — map/forEach release it
//! after the call, filter transfers the kept ones into the result.
//! The callback's return is +1-owned: map transfers it into the
//! result slot, filter/forEach release it. A pending throw after any
//! callback aborts the loop, releases the partial result, and
//! returns `undefined` for the SSA-side throw check to propagate
//! (the merge-sort comparator poll, sort.rs, is the precedent).
//!
//! Results are fresh `Arr<Any>` blocks (NaN-box slots are the only
//! self-describing representation for runtime-mixed callback
//! outputs).

use core::ffi::c_void;

use crate::layout::ARR_LEN_OFF;

unsafe extern "C" {
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
    fn __torajs_anyv_unbox_tag(v: u64) -> i64;
    fn __torajs_anyv_unbox_value(v: u64) -> i64;
    /// torajs-anyvalue — ES ToBoolean over an AnyValue (filter's
    /// predicate coercion).
    fn __torajs_anyv_to_bool(v: u64) -> bool;
    /// Cross-tier — universal NaN-box-safe heap dropper.
    fn __torajs_value_drop_heap(p: *mut c_void);
    /// Cross-tier — torajs-rc. NaN-box-safe refcount bump.
    fn __torajs_rc_inc(p: *mut c_void);
    /// Cross-tier — torajs-throw. Non-zero iff a throw is pending.
    fn __torajs_throw_check() -> i64;
}

/// The boxed dual-entry ABI (torajs-core `ssa_lower_boxed_entry`).
type BoxedFn = unsafe extern "C" fn(*mut c_void, *const u64, i64) -> u64;

/// Adapter argv width mirror (`MAX_BOXED_PARAMS`).
const ARGV_SLOTS: usize = 8;

/// NaN-box `undefined`.
#[inline]
unsafe fn undef() -> u64 {
    unsafe { __torajs_anyv_box_from_pair(5, 0) }
}

/// RFC 20260717-objlit-anylane-recv knife 2e — 1 when the callback
/// cell declares the receiver-first channel (flags bit 12: an
/// any-lane literal method whose body says `this`), else 0. The HOF
/// argv shifts `(v, k, O)` up one slot so the promoted body's
/// `__this` param reads the buffer's `undefined` padding (no-thisArg
/// callbacks bind `this = undefined`). Read once per walk.
#[inline]
pub(crate) unsafe fn recv_first_shift(cb_env: *mut c_void) -> usize {
    unsafe {
        let flags = (cb_env as *const u8).add(6).cast::<u16>().read();
        usize::from(flags & torajs_rc::FLAG_CLOSURE_RECV_FIRST != 0)
    }
}

/// Shared HO loop. `mode`: 0 = forEach (result `undefined`),
/// 1 = map, 2 = filter.
unsafe fn hof_loop(
    arr: *const c_void,
    cb_env: *mut c_void,
    cb_entry: u64,
    mode: i64,
    this_arg: u64,
) -> u64 {
    unsafe {
        // RFC 20260810 刀 D — a sparse tail would spin ~len rounds
        // and allocate a len-sized result; loud reject until the
        // iteration methods grow real sparse support.
        if crate::sparse_gate::sparse_tail_rejects(
            arr,
            b"sparse array tail is not yet supported in Array.prototype.map/filter/forEach\0"
                .as_ptr(),
        ) {
            return undef();
        }
        let cb: BoxedFn = core::mem::transmute(cb_entry as usize);
        let s = recv_first_shift(cb_env);
        let len = *((arr as *const u8).add(ARR_LEN_OFF) as *const u64);
        let out: *mut u8 = if mode == 0 {
            core::ptr::null_mut()
        } else {
            crate::alloc::__torajs_arr_alloc_any(len)
        };
        // The receiver rides as the callback's third argument — a
        // heap cell's NaN-box encoding is its pointer bits (borrow).
        let arr_boxed = arr as u64;
        let mut i: u64 = 0;
        while i < len {
            // §23.1.3.15 step 4.b / §23.1.3.21 step 6.b — the loop
            // asks HasProperty before it calls anything, so a hole
            // nothing on the chain supplies is SKIPPED, not visited
            // with `undefined`. This lane had no such gate: it read
            // every index and called the callback on it.
            //
            // The typed lane has always had one, which is why the
            // shape only shows through the ANY lane — and a file
            // reaches that lane just by naming `Object` or `Array`
            // as a value, which is ordinary enough that
            // `arr-hole-proto-has-001` was green only because an AST
            // rewrite happened to erase its `Object.prototype`
            // mention. `__torajs_arr_has_index` is the same §7.3.11
            // kernel the `in` lane runs, prototype digit keys
            // included.
            //
            // map (mode 1) is NOT gated here: skipping would shorten
            // its result, and the spec wants a hole in that position
            // instead. Recorded rather than half-done — the callback
            // still runs for a hole there.
            if mode != 1
                && crate::define_hole::__torajs_arr_has_index(arr as *mut c_void, i as i64) == 0
            {
                i += 1;
                continue;
            }
            // Kind-aware boxed read — +1-owned for cells.
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
                // Abort: release this round's values and the partial
                // result; the dispatcher's caller propagates.
                __torajs_value_drop_heap(v as *mut c_void);
                __torajs_value_drop_heap(r as *mut c_void);
                if !out.is_null() {
                    __torajs_value_drop_heap(out as *mut c_void);
                }
                return undef();
            }
            match mode {
                1 => {
                    // map — transfer the owned return into the slot.
                    let tag = __torajs_anyv_unbox_tag(r);
                    let value = __torajs_anyv_unbox_value(r);
                    crate::any::__torajs_arr_push_any(out as *mut c_void, tag as u64, value as u64);
                    __torajs_value_drop_heap(v as *mut c_void);
                }
                2 => {
                    // filter — keep transfers the owned element read.
                    let keep = __torajs_anyv_to_bool(r);
                    __torajs_value_drop_heap(r as *mut c_void);
                    if keep {
                        let tag = __torajs_anyv_unbox_tag(v);
                        let value = __torajs_anyv_unbox_value(v);
                        crate::any::__torajs_arr_push_any(
                            out as *mut c_void,
                            tag as u64,
                            value as u64,
                        );
                    } else {
                        __torajs_value_drop_heap(v as *mut c_void);
                    }
                }
                _ => {
                    // forEach — both values release.
                    __torajs_value_drop_heap(r as *mut c_void);
                    __torajs_value_drop_heap(v as *mut c_void);
                }
            }
            i += 1;
        }
        if out.is_null() { undef() } else { out as u64 }
    }
}

/// `a.map(cb)` per ES §23.1.3.19 — fresh `Arr<Any>` of the callback
/// returns.
///
/// # Safety
/// `arr` is a valid `Tag::Arr` heap pointer; `(cb_env, cb_entry)` is
/// a live closure cell + its non-zero boxed dual entry.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_any_map(
    arr: *const c_void,
    cb_env: *mut c_void,
    cb_entry: u64,
    this_arg: u64,
) -> u64 {
    unsafe { hof_loop(arr, cb_env, cb_entry, 1, this_arg) }
}

/// `a.filter(cb)` per ES §23.1.3.8 — fresh `Arr<Any>` of the
/// elements whose predicate coerced true.
///
/// # Safety
/// See [`__torajs_arr_any_map`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_any_filter(
    arr: *const c_void,
    cb_env: *mut c_void,
    cb_entry: u64,
    this_arg: u64,
) -> u64 {
    unsafe { hof_loop(arr, cb_env, cb_entry, 2, this_arg) }
}

/// `a.forEach(cb)` per ES §23.1.3.15 — side effects only, returns
/// `undefined`.
///
/// # Safety
/// See [`__torajs_arr_any_map`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_any_for_each(
    arr: *const c_void,
    cb_env: *mut c_void,
    cb_entry: u64,
    this_arg: u64,
) -> u64 {
    unsafe { hof_loop(arr, cb_env, cb_entry, 0, this_arg) }
}

/// Shared early-exit predicate loop (any-dispatch backfill chunk 3).
/// `mode`: 0 = every, 1 = some, 2 = find, 3 = findIndex,
/// 4 = findLast, 5 = findLastIndex (modes ≥ 4 walk backwards,
/// §23.1.3.11 / §23.1.3.12). Same ledger as [`hof_loop`]; on the
/// exit hit `find` / `findLast` transfer the owned element read as
/// the return, the others release it.
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
/// See [`__torajs_arr_any_map`].
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
/// See [`__torajs_arr_any_map`].
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
/// See [`__torajs_arr_any_map`].
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
/// See [`__torajs_arr_any_map`].
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
/// See [`__torajs_arr_any_map`].
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
/// See [`__torajs_arr_any_map`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_any_find_last_index(
    arr: *const c_void,
    cb_env: *mut c_void,
    cb_entry: u64,
    this_arg: u64,
) -> u64 {
    unsafe { find_loop(arr, cb_env, cb_entry, 5, this_arg) }
}

/// `a.reduce(cb, init?)` / `a.reduceRight(cb, init?)` per ES
/// §23.1.3.24 / §23.1.3.25 — `right` flips the walk. The callback
/// receives `(acc, value, index, array)`. `init` is BORROWED
/// (+1 taken here when present); without one the seed is the first
/// walked element (owned read), and an empty array raises the
/// spec TypeError through the existing throw_empty sentinels. The
/// returned accumulator is +1-owned (the callback's owned return
/// carries through; the seed paths stake their own).
///
/// # Safety
/// See [`__torajs_arr_any_map`]; `init` is a valid NaN-box AnyValue
/// when `has_init != 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_any_reduce(
    arr: *const c_void,
    cb_env: *mut c_void,
    cb_entry: u64,
    init: u64,
    has_init: i64,
    right: i64,
) -> u64 {
    unsafe {
        // RFC 20260810 刀 D — same loud reject as `hof_loop`.
        if crate::sparse_gate::sparse_tail_rejects(
            arr,
            b"sparse array tail is not yet supported in Array.prototype.reduce\0".as_ptr(),
        ) {
            return undef();
        }
        let cb: BoxedFn = core::mem::transmute(cb_entry as usize);
        let s = recv_first_shift(cb_env);
        let len = *((arr as *const u8).add(ARR_LEN_OFF) as *const u64) as i64;
        let arr_boxed = arr as u64;
        let step: i64 = if right != 0 { -1 } else { 1 };
        let mut i: i64 = if right != 0 { len - 1 } else { 0 };
        let mut acc: u64;
        if has_init != 0 {
            __torajs_rc_inc(init as *mut c_void);
            acc = init;
        } else {
            if len == 0 {
                if right != 0 {
                    crate::throw_empty::__torajs_arr_throw_reduce_right_empty();
                } else {
                    crate::throw_empty::__torajs_arr_throw_reduce_empty();
                }
                return undef();
            }
            acc = crate::index_any::__torajs_arr_index_get(arr, i);
            i += step;
        }
        while i >= 0 && i < len {
            let v = crate::index_any::__torajs_arr_index_get(arr, i);
            let mut argv = [undef(); ARGV_SLOTS];
            argv[s] = acc;
            argv[s + 1] = v;
            argv[s + 2] = __torajs_anyv_box_from_pair(2, i);
            argv[s + 3] = arr_boxed;
            let r = cb(cb_env, argv.as_ptr(), (4 + s) as i64);
            __torajs_value_drop_heap(v as *mut c_void);
            __torajs_value_drop_heap(acc as *mut c_void);
            if __torajs_throw_check() != 0 {
                __torajs_value_drop_heap(r as *mut c_void);
                return undef();
            }
            acc = r;
            i += step;
        }
        acc
    }
}
