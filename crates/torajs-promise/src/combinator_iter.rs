//! Iterator-interleaved dyn combinators — RFC
//! 20260810-promise-iterator-interleave knives I1-I3.
//!
//! The collect-then-delegate shape next door drives the whole
//! iterable before any element is looked at. That has no answer for
//! an infinite iterable, and it runs the per-element `then`
//! GET/INVOKE (§27.2.4.1.3 step 6.q-s and its race / allSettled /
//! any mirrors) — an observable user hook — after the iterator is
//! already exhausted, so test262's invoke-then-error-close family
//! (throw out of the first element's `then`, assert the iterator was
//! CLOSED once) sat in the timeout column.
//!
//! This lane iterates the spec's way: one loop, and inside it each
//! element is observed. An element promise wearing a user `then`
//! override (the expando-bag landing knife 1 stored) ACTIVATES the
//! fan-in block and hands the override a freshly minted
//! resolveElement / rejectElement pair (`combinator_iter_settle`
//! picks the mode's pair); its throw — or a `then` getter's — closes
//! the iterator per §27.2.4.1 step 8.a and rejects the outer with
//! the original abrupt. A loop that never activates falls through to
//! the exact collect-then-delegate call it replaced, so every
//! settled-input tick position recorded by existing fixtures is
//! untouched.
//!
//! The block's `remaining` runs the spec's own growable protocol
//! here: it starts at 1 (the iteration-in-progress sentinel), each
//! waited-on element adds one, and the loop's normal exit subtracts
//! the sentinel — reaching zero is "every element reported in AND
//! the iteration is over" (§27.2.4.1.3 steps 6.d.iii / 6.o). `race`
//! opts out: nothing is collected and nothing finishes — the first
//! settlement wins and an empty iterable stays pending forever
//! (§27.2.4.5 step 3).

use core::ffi::c_void;

use crate::combinator_all_fanin::{
    AllBlock, MODE_ALL, MODE_ALLSETTLED, MODE_ANY, MODE_RACE, attach_elem_job, release_block,
    wrap_plain,
};
use crate::combinator_dyn::{IterStepFn, reject_with_pending_throw};
use crate::layout::{REPR_ANY, REPR_HEAP, REPR_UNSTAMPED, as_promise};

unsafe extern "C" {
    #[link_name = "__torajs_libc_malloc"]
    fn malloc(n: usize) -> *mut c_void;
    fn __torajs_rc_inc(p: *mut c_void);
    fn __torajs_value_drop_heap(p: *mut c_void);
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
    fn __torajs_anyv_rc_dec(v: u64);
    fn __torajs_throw_check() -> i64;
    fn __torajs_throw_take() -> i64;
    fn __torajs_throw_take_tag() -> i64;
    fn __torajs_arr_push(arr: *mut c_void, val: i64) -> *mut c_void;
    /// torajs-anyvalue — per-element `then` GET over the promise's
    /// expando bag (0 = no override; owned box otherwise; a getter
    /// throw stays pending with 0 returned).
    fn __torajs_promise_then_observed(cell: *mut c_void) -> u64;
    /// torajs-anyvalue — `f(args…)` over an any-boxed callee.
    fn __torajs_any_call(recv: u64, argv: *const u64, argc: i64) -> u64;
    /// torajs-anyvalue — the explicit-`this` twin: the heir lane's
    /// per-element `Call(promiseResolve, C, «v»)` needs C as the
    /// thisArg (§27.2.4.1.3 step 6.i), and a recv-first builtin
    /// static cell reads it from there.
    fn __torajs_any_call_with_this(recv: u64, this_arg: u64, argv: *const u64, argc: i64) -> u64;
    /// torajs-anyvalue — §7.4.9 IteratorClose under a pending throw
    /// (stash, close, swallow the close's own throw, restore).
    fn __torajs_iter_close_abrupt(iter: u64);
    fn __torajs_promise_reject(p: *mut c_void, reason: i64);
    /// torajs-anyvalue — one patched-`Promise.resolve` invocation
    /// (arg borrowed, owned Any answer, throw left pending).
    fn __torajs_promise_call_patched(name_id: i64, arg: u64) -> u64;
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

/// The undefined box — no stake to account for.
unsafe fn undef() -> u64 {
    unsafe { __torajs_anyv_box_from_pair(5, 0) }
}

/// One interleaved combinator run. `step` is the for-of any-lane
/// protocol fn (GetIterator + IteratorStepValue folded); `sync` is
/// where the never-activated exit delegates. A builtin-heir run
/// (`Promise.all.call(CP, xs)` — the recv-first static arm) passes
/// its class object and the GetPromiseResolve(C) answer in `ctor` /
/// `resolve_f` (both BORROWED, 0/0 on the builtin path): every
/// element then runs `Call(resolve_f, C, «v»)` in the loop, and the
/// builtin `Promise.resolve` patch lane stays silent — C's own
/// `resolve` is the only observable per §27.2.4.1.2.
pub(crate) unsafe fn dyn_iter(
    v: u64,
    step: IterStepFn,
    mode: u8,
    sync: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    ctor: u64,
    resolve_f: u64,
) -> *mut c_void {
    unsafe {
        let heir = resolve_f != 0;
        // §27.2.4.1 step 1 GetPromiseResolve — the patch probe runs
        // once, before the first step, like the spec's single fetch
        // (the patched fn itself is re-read per call; a data write
        // is not observable through either read). The heir lane
        // already carries its own promiseResolve, so the builtin
        // probe never fires on it.
        let patched = !heir && crate::combinator_patched::consult_active();
        let mut idx: i64 = 0;
        let mut iter_slot: u64 = undef();
        let mut out_v: u64 = undef();
        // Collected while no element has demanded interleaving; the
        // never-activated exit hands these to the sync kernel so its
        // tick positions stay exactly where the fixtures put them.
        let mut items: Vec<u64> = Vec::new();
        let mut b: *mut AllBlock = core::ptr::null_mut();
        let mut next_index: u64 = 0;
        loop {
            let has = step(v, &mut idx, &mut iter_slot, &mut out_v);
            if __torajs_throw_check() != 0 {
                // IteratorStepValue's own abrupt — [[Done]] is true,
                // §27.2.4.1 step 8.a does NOT close.
                for it in items {
                    __torajs_anyv_rc_dec(it);
                }
                __torajs_anyv_rc_dec(iter_slot);
                return if b.is_null() {
                    reject_with_pending_throw()
                } else {
                    reject_result_with_pending(b)
                };
            }
            if has == 0 {
                break;
            }
            let mut elem = out_v;
            // §27.2.4.1.3 step 6.i — `Call(promiseResolve, C,
            // «nextValue»)` runs INSIDE the loop when the slot wears
            // a user patch or the run is a heir's: its abrupt — or a
            // non-promise answer, the typed lane's recorded return
            // contract — closes the iterator per §27.2.4.1 step 8.a
            // and rejects with the original completion. The sync
            // kernels' consult fires only after full collection,
            // which has no answer for an infinite iterable; this one
            // does.
            if patched || heir {
                let ret = if heir {
                    let one = [elem];
                    __torajs_any_call_with_this(resolve_f, ctor, one.as_ptr(), 1)
                } else {
                    __torajs_promise_call_patched(0, elem)
                };
                __torajs_anyv_rc_dec(elem);
                if __torajs_throw_check() == 0 && crate::combinator_any::slot_promise(ret).is_none()
                {
                    __torajs_anyv_rc_dec(ret);
                    __torajs_throw_type_error(
                        c"the patched Promise.resolve answered a non-promise into a typed slot"
                            .as_ptr(),
                    );
                }
                if __torajs_throw_check() != 0 {
                    return close_and_reject(items, iter_slot, b);
                }
                elem = ret;
            }
            // Per-element `then` observation — promise cells only
            // (a plain value's promiseResolve wrapper wears the
            // builtin `then`; the user-thenable protocol is the
            // codebase-wide PromiseResolveThenableJob gap).
            if let Some(pp) = crate::combinator_any::slot_promise(elem) {
                (*pp).has_handler = 1;
                let user_then = __torajs_promise_then_observed(pp as *mut c_void);
                if __torajs_throw_check() != 0 {
                    // then GET threw (step 6.q via the getter) —
                    // close the iterator, original wins.
                    __torajs_anyv_rc_dec(elem);
                    return close_and_reject(items, iter_slot, b);
                }
                if user_then != 0 {
                    if b.is_null() {
                        b = activate(mode, &mut items, &mut next_index);
                    }
                    let index = alloc_index(b, mode, &mut next_index);
                    invoke_user_then(b, mode, index, user_then);
                    __torajs_anyv_rc_dec(user_then);
                    __torajs_anyv_rc_dec(elem);
                    if __torajs_throw_check() != 0 {
                        // then INVOKE threw (step 6.s) — same close.
                        return close_and_reject(items, iter_slot, b);
                    }
                    continue;
                }
            }
            if b.is_null() {
                items.push(elem);
            } else {
                let index = alloc_index(b, mode, &mut next_index);
                attach_elem_bits(b, mode, index, elem);
            }
        }
        __torajs_anyv_rc_dec(iter_slot);
        if b.is_null() {
            // Never activated — the exact delegate this lane
            // replaced. A patched or heir run already resolved every
            // element above, so it hands the collected promises
            // straight to the any-lane sibling: the `no_mangle` sync
            // entry would probe the patch again and run resolve
            // twice per element.
            let arr = crate::combinator_dyn::items_to_any_arr(items);
            let out = if patched || heir {
                match mode {
                    MODE_ALL => crate::combinator_any::all_sync_any(arr),
                    MODE_RACE => crate::combinator_any::race_sync_any(arr),
                    MODE_ANY => crate::combinator_any::any_sync_any(arr),
                    // allSettled — the dyn entry has no record tags
                    // to give (`allsettled_sync_untagged` posture).
                    _ => crate::combinator_any::allsettled_sync_any(arr, 0),
                }
            } else {
                sync(arr)
            };
            __torajs_value_drop_heap(arr);
            return out;
        }
        // §27.2.4.1.3 step 6.d.iii — the iteration is done: drop the
        // sentinel, and zero means every element already reported in.
        // `race` never finishes: first settlement or forever pending.
        if mode != MODE_RACE {
            (*b).remaining -= 1;
            if (*b).remaining == 0 && (*b).done == 0 {
                crate::combinator_fanin_slot::finish(b);
            }
        }
        let result = (*b).result;
        release_block(b);
        result
    }
}

/// First interleaving element — mint the block and re-home what the
/// collect phase already gathered. The alloc's original stake is the
/// caller's return; the block holds its own. `race` collects nothing
/// (no result array, no counter — the outer settles or stays put);
/// everything else runs the sentinel protocol from the module doc.
unsafe fn activate(mode: u8, items: &mut Vec<u64>, next_index: &mut u64) -> *mut AllBlock {
    unsafe {
        let result = crate::pool::__torajs_promise_alloc_pending();
        let rp = as_promise(result);
        // Everything the interleaved lane settles with is boxed —
        // race / any forward one boxed value, all / allSettled
        // resolve with a heap array.
        (*rp).value_repr = if mode == MODE_ALL || mode == MODE_ALLSETTLED {
            REPR_HEAP
        } else {
            REPR_ANY
        };
        (*rp).value_is_heap = 1;
        let b = malloc(core::mem::size_of::<AllBlock>()) as *mut AllBlock;
        (*b).result = result;
        (*b).result_arr = if mode == MODE_RACE {
            core::ptr::null_mut()
        } else {
            // any-shape for all's values and any's errors; raw slots
            // for allSettled's record cells. All grow by push.
            crate::combinator_any::alloc_any_result(0)
        };
        // The iteration-in-progress sentinel (module doc).
        (*b).remaining = 1;
        (*b).jobs = 1;
        (*b).elem_repr = if mode == MODE_ALLSETTLED {
            REPR_HEAP
        } else {
            REPR_ANY
        };
        (*b).done = 0;
        (*b).mode = mode;
        (*b).record_tags = 0;
        (*b).record_value_repr = if mode == MODE_ALLSETTLED {
            REPR_ANY
        } else {
            REPR_UNSTAMPED
        };
        (*b).input_any = 1;
        (*b).result_any = u8::from(mode == MODE_ALL);
        __torajs_rc_inc(result);
        for bits in items.drain(..) {
            let index = alloc_index(b, mode, next_index);
            attach_elem_bits(b, mode, index, bits);
        }
        b
    }
}

/// Claim the next element index and back it with a result slot —
/// every index is backed before anything can write it, and the array
/// cell is stable across the grow (RFC 20260706 B1), so the
/// data-pointer walk in `store_slot` stays valid. `race` has no
/// result array and no indices worth distinguishing.
unsafe fn alloc_index(b: *mut AllBlock, mode: u8, next_index: &mut u64) -> u64 {
    unsafe {
        if mode != MODE_RACE {
            (*b).result_arr = __torajs_arr_push((*b).result_arr, undef() as i64);
        }
        let index = *next_index;
        *next_index += 1;
        index
    }
}

/// One non-interleaving element, owned bits in.
///
/// The waited-on shapes take the counter up (§27.2.4.1.3 step 6.o)
/// and their settle path takes it back down; a plain element of
/// `all` / `allSettled` parks at once (net zero). `any` wraps a
/// plain element per §27.2.4.2 (the deciding tick is observable);
/// `race` adopts — first settlement wins, and a plain element rides
/// an already-fulfilled wrapper through the same adopt.
unsafe fn attach_elem_bits(b: *mut AllBlock, mode: u8, index: u64, bits: u64) {
    unsafe {
        if mode == MODE_RACE {
            let pp = match crate::combinator_any::slot_promise(bits) {
                Some(pp) => {
                    (*pp).has_handler = 1;
                    // The owned box IS one stake on the cell — it
                    // rides into the adopt, so no inc and no dec.
                    pp
                }
                None => {
                    let w = wrap_plain(bits);
                    // The wrapper carries its own stake; the element
                    // box's is released (adopt consumes the wrapper's).
                    __torajs_anyv_rc_dec(bits);
                    w
                }
            };
            crate::then_adopt::adopt_into((*b).result, pp as *mut c_void);
            return;
        }
        match crate::combinator_any::slot_promise(bits) {
            Some(pp) => {
                (*pp).has_handler = 1;
                (*b).remaining += 1;
                attach_elem_job(b, pp, index);
            }
            None if mode == MODE_ANY => {
                // §27.2.4.2 wraps a plain element so its deciding
                // tick stays observable — a job like every other.
                let w = wrap_plain(bits);
                __torajs_anyv_rc_dec(bits);
                (*b).remaining += 1;
                attach_elem_job(b, w, index);
            }
            None => {
                crate::combinator_fanin_slot::store_plain(b, index, bits);
                __torajs_anyv_rc_dec(bits);
            }
        }
    }
}

/// The abrupt exit both observation throws take: release what the
/// collect phase holds, close the iterator with the throw stashed
/// (§27.2.4.1 step 8.a — the original completion wins over the
/// close's own), and reject.
unsafe fn close_and_reject(items: Vec<u64>, iter_slot: u64, b: *mut AllBlock) -> *mut c_void {
    unsafe {
        for it in items {
            __torajs_anyv_rc_dec(it);
        }
        __torajs_iter_close_abrupt(iter_slot);
        __torajs_anyv_rc_dec(iter_slot);
        if b.is_null() {
            reject_with_pending_throw()
        } else {
            reject_result_with_pending(b)
        }
    }
}

/// The activated twin of `reject_with_pending_throw` — the outer
/// already exists, so the pending throw settles IT. The alloc's
/// original stake carries out as the return.
unsafe fn reject_result_with_pending(b: *mut AllBlock) -> *mut c_void {
    unsafe {
        let tag = __torajs_throw_take_tag();
        let value = __torajs_throw_take();
        let boxed = __torajs_anyv_box_from_pair(tag, value);
        let rp = as_promise((*b).result);
        (*rp).value_repr = REPR_ANY;
        (*rp).value_is_heap = 1;
        (*b).done = 1;
        __torajs_promise_reject((*b).result, boxed as i64);
        let result = (*b).result;
        release_block(b);
        result
    }
}

/// §27.2.4.1.3 step 6.r — Invoke the user override with the mode's
/// resolveElement / rejectElement pair. Each cell holds a block
/// stake (`jobs`); the mint stakes die right after the call unless
/// the user body kept them. The waited-on count goes up for every
/// mode that counts — the pair's settle entries take it back down.
unsafe fn invoke_user_then(b: *mut AllBlock, mode: u8, index: u64, user_then: u64) {
    unsafe {
        if mode != MODE_RACE {
            (*b).remaining += 1;
        }
        let (res_entry, rej_entry) = crate::combinator_iter_settle::entries_for(mode);
        let rcell = crate::combinator_iter_settle::mint_resolver(b, index, res_entry);
        let jcell = crate::combinator_iter_settle::mint_resolver(b, index, rej_entry);
        let argv = [
            __torajs_anyv_box_from_pair(4, rcell as i64),
            __torajs_anyv_box_from_pair(4, jcell as i64),
        ];
        let ret = __torajs_any_call(user_then, argv.as_ptr(), 2);
        let threw = __torajs_throw_check() != 0;
        __torajs_value_drop_heap(rcell);
        __torajs_value_drop_heap(jcell);
        if !threw {
            __torajs_anyv_rc_dec(ret);
        }
    }
}
