//! Iterator-interleaved `Promise.all` over a dynamic argument — RFC
//! 20260810-promise-iterator-interleave knives I1+I2.
//!
//! The collect-then-delegate shape next door drives the whole
//! iterable before any element is looked at. That has no answer for
//! an infinite iterable, and it runs the per-element `then`
//! GET/INVOKE (§27.2.4.1.3 step 6.q-s) — an observable user hook —
//! after the iterator is already exhausted, so test262's
//! invoke-then-error-close family (throw out of the first element's
//! `then`, assert the iterator was CLOSED once) sat in the timeout
//! column.
//!
//! This lane iterates the spec's way: one loop, and inside it each
//! element is observed. An element promise wearing a user `then`
//! override (the expando-bag landing knife 1 stored) ACTIVATES the
//! fan-in block and hands the override a freshly minted
//! resolveElement / reject pair (§27.2.4.1.3 steps 6.o-r; the
//! `Promise.withResolvers` cell shape); its throw — or a `then`
//! getter's — closes the iterator per §27.2.4.1 step 8.a and rejects
//! the outer with the original abrupt. A loop that never activates
//! falls through to the exact collect-then-delegate call it replaced,
//! so every settled-input tick position recorded by existing
//! fixtures is untouched.
//!
//! The block's `remaining` runs the spec's own growable protocol
//! here: it starts at 1 (the iteration-in-progress sentinel), each
//! waited-on element adds one, and the loop's normal exit subtracts
//! the sentinel — reaching zero is "every element reported in AND
//! the iteration is over", which is exactly §27.2.4.1.3 steps 6.d.iii
//! / 6.o / §27.2.4.1.2 step 10.

use core::ffi::c_void;

use crate::combinator_all_fanin::{AllBlock, MODE_ALL, attach_elem_job, release_block};
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
    /// torajs-anyvalue — §7.4.9 IteratorClose under a pending throw
    /// (stash, close, swallow the close's own throw, restore).
    fn __torajs_iter_close_abrupt(iter: u64);
    fn __torajs_promise_reject(p: *mut c_void, reason: i64);
    /// torajs-cycle — scrub a dying cell from the root buffer.
    fn __torajs_cycle_unbuffer(p: *mut c_void);
}

/// The undefined box — no stake to account for.
unsafe fn undef() -> u64 {
    unsafe { __torajs_anyv_box_from_pair(5, 0) }
}

/// §27.2.4.1 through the interleaved loop. `step` is the for-of
/// any-lane protocol fn (GetIterator + IteratorStepValue folded).
pub(crate) unsafe fn all_dyn_iter(v: u64, step: IterStepFn) -> *mut c_void {
    unsafe {
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
            let elem = out_v;
            // Per-element `then` observation — promise cells only
            // (a plain value's promiseResolve wrapper wears the
            // builtin `then`; the user-thenable protocol is the
            // codebase-wide PromiseResolveThenableJob gap).
            if let Some(pp) = crate::combinator_any::slot_promise(elem) {
                (*pp).has_handler = 1;
                let user_then = __torajs_promise_then_observed(pp as *mut c_void);
                if __torajs_throw_check() != 0 {
                    // then GET threw (§27.2.4.1.3 step 6.q via the
                    // getter) — close the iterator, original wins.
                    __torajs_anyv_rc_dec(elem);
                    return close_and_reject(items, iter_slot, b);
                }
                if user_then != 0 {
                    if b.is_null() {
                        b = activate(&mut items, &mut next_index);
                    }
                    push_placeholder(b);
                    let index = next_index;
                    next_index += 1;
                    (*b).remaining += 1;
                    invoke_user_then(b, index, user_then);
                    __torajs_anyv_rc_dec(user_then);
                    __torajs_anyv_rc_dec(elem);
                    if __torajs_throw_check() != 0 {
                        // then INVOKE threw (step 6.s) — same close.
                        return close_and_reject(Vec::new(), iter_slot, b);
                    }
                    continue;
                }
            }
            if b.is_null() {
                items.push(elem);
            } else {
                push_placeholder(b);
                let index = next_index;
                next_index += 1;
                attach_elem_bits(b, index, elem);
            }
        }
        __torajs_anyv_rc_dec(iter_slot);
        if b.is_null() {
            // Never activated — the exact delegate this lane replaced.
            let arr = crate::combinator_dyn::items_to_any_arr(items);
            let out = crate::combinator_dyn::all_sync_untargeted(arr);
            __torajs_value_drop_heap(arr);
            return out;
        }
        // §27.2.4.1.3 step 6.d.iii — the iteration is done: drop the
        // sentinel, and zero means every element already reported in.
        (*b).remaining -= 1;
        if (*b).remaining == 0 && (*b).done == 0 {
            crate::combinator_fanin_slot::finish(b);
        }
        let result = (*b).result;
        release_block(b);
        result
    }
}

/// First interleaving element — mint the block and re-home what the
/// collect phase already gathered. The alloc's original stake is the
/// caller's return; the block holds its own.
unsafe fn activate(items: &mut Vec<u64>, next_index: &mut u64) -> *mut AllBlock {
    unsafe {
        let result = crate::pool::__torajs_promise_alloc_pending();
        let rp = as_promise(result);
        (*rp).value_repr = REPR_HEAP;
        (*rp).value_is_heap = 1;
        let b = malloc(core::mem::size_of::<AllBlock>()) as *mut AllBlock;
        (*b).result = result;
        (*b).result_arr = crate::combinator_any::alloc_any_result(0);
        // The iteration-in-progress sentinel (module doc).
        (*b).remaining = 1;
        (*b).jobs = 1;
        (*b).elem_repr = REPR_ANY;
        (*b).done = 0;
        (*b).mode = MODE_ALL;
        (*b).record_tags = 0;
        (*b).record_value_repr = REPR_UNSTAMPED;
        (*b).input_any = 1;
        (*b).result_any = 1;
        __torajs_rc_inc(result);
        for bits in items.drain(..) {
            push_placeholder(b);
            let index = *next_index;
            *next_index += 1;
            attach_elem_bits(b, index, bits);
        }
        b
    }
}

/// Grow the result array by one undefined slot — every index is
/// backed before anything can write it, and the cell is stable
/// across the grow (RFC 20260706 B1), so `store_slot`'s data-pointer
/// walk stays valid.
unsafe fn push_placeholder(b: *mut AllBlock) {
    unsafe {
        (*b).result_arr = __torajs_arr_push((*b).result_arr, undef() as i64);
    }
}

/// One non-interleaving element, owned bits in: a promise element's
/// stake transfers into its job; a plain value parks in its slot per
/// §27.2.4.1.3 step 6.h's already-fulfilled reading (net-zero on the
/// counter: step 6.o's increment and its immediate settle cancel).
unsafe fn attach_elem_bits(b: *mut AllBlock, index: u64, bits: u64) {
    unsafe {
        match crate::combinator_any::slot_promise(bits) {
            Some(pp) => {
                (*pp).has_handler = 1;
                (*b).remaining += 1;
                // The owned box IS one stake on the cell — it rides
                // into the job, so no inc and no dec.
                attach_elem_job(b, pp, index);
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

// ---- the resolveElement / reject pair handed to a user `then` ----
//
// Closure-cell layout mirror (ssa_lower closure-env / the
// `promise_with_resolvers` mint shape, lockstep):

const CLOSURE_TAG: u16 = 3;
const C_FN_ADDR_OFF: usize = 8;
const C_DROP_FN_OFF: usize = 16;
const C_PROPS_OFF: usize = 24;
const C_BOXED_ENTRY_OFF: usize = 32;
const C_TRACE_FN_OFF: usize = 40;
const C_BLOCK_OFF: usize = 48;
const C_INDEX_OFF: usize = 56;
const C_ALREADY_OFF: usize = 64;
const RESOLVER_CELL_SIZE: usize = 72;

/// §27.2.4.1.3 step 6.r — Invoke the user override with a fresh
/// resolveElement / reject pair. Each cell holds a block stake
/// (`jobs`); the mint stakes die right after the call unless the
/// user body kept them.
unsafe fn invoke_user_then(b: *mut AllBlock, index: u64, user_then: u64) {
    unsafe {
        let rcell = mint_resolver(b, index, iter_resolve_elem_entry);
        let jcell = mint_resolver(b, index, iter_reject_entry);
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

/// One settle-function cell over the shared block — the
/// `mint_resolver` shape next crate over, captures `(block, index)`.
unsafe fn mint_resolver(
    b: *mut AllBlock,
    index: u64,
    entry: unsafe extern "C" fn(*mut c_void, *const u64, i64) -> u64,
) -> *mut c_void {
    unsafe {
        let cell = malloc(RESOLVER_CELL_SIZE) as *mut u8;
        *(cell as *mut u32) = 1;
        *(cell.add(4) as *mut u16) = CLOSURE_TAG;
        *(cell.add(6) as *mut u16) = 0;
        *(cell.add(C_FN_ADDR_OFF) as *mut u64) = iter_resolver_native_entry as *const () as u64;
        *(cell.add(C_DROP_FN_OFF) as *mut u64) = iter_resolver_drop as *const () as u64;
        *(cell.add(C_PROPS_OFF) as *mut u64) = 0;
        *(cell.add(C_BOXED_ENTRY_OFF) as *mut u64) = entry as *const () as u64;
        *(cell.add(C_TRACE_FN_OFF) as *mut u64) = iter_resolver_trace as *const () as u64;
        *(cell.add(C_BLOCK_OFF) as *mut u64) = b as u64;
        *(cell.add(C_INDEX_OFF) as *mut u64) = index;
        *(cell.add(C_ALREADY_OFF) as *mut u8) = 0;
        (*b).jobs += 1;
        cell as *mut c_void
    }
}

/// §27.2.4.1.2 Promise.all resolve-element: once per cell
/// ([[AlreadyCalled]]), park the value, count out, finish at zero.
unsafe extern "C" fn iter_resolve_elem_entry(env: *mut c_void, argv: *const u64, argc: i64) -> u64 {
    unsafe {
        let cell = env.cast::<u8>();
        if *cell.add(C_ALREADY_OFF) != 0 {
            return undef();
        }
        *cell.add(C_ALREADY_OFF) = 1;
        let b = *(cell.add(C_BLOCK_OFF) as *const u64) as *mut AllBlock;
        if (*b).done == 0 {
            let v = if argc >= 1 { *argv } else { undef() };
            let index = *(cell.add(C_INDEX_OFF) as *const u64);
            crate::combinator_fanin_slot::store_plain(b, index, v);
            (*b).remaining -= 1;
            if (*b).remaining == 0 {
                crate::combinator_fanin_slot::finish(b);
            }
        }
        undef()
    }
}

/// The pair's reject face — the result capability's [[Reject]]:
/// first settle wins, the reason rides boxed.
unsafe extern "C" fn iter_reject_entry(env: *mut c_void, argv: *const u64, argc: i64) -> u64 {
    unsafe {
        let cell = env.cast::<u8>();
        let b = *(cell.add(C_BLOCK_OFF) as *const u64) as *mut AllBlock;
        if (*b).done == 0 {
            let v = if argc >= 1 { *argv } else { undef() };
            crate::combinator_any::box_share(v);
            let rp = as_promise((*b).result);
            (*rp).value_repr = REPR_ANY;
            (*rp).value_is_heap = 1;
            (*b).done = 1;
            __torajs_promise_reject((*b).result, v as i64);
        }
        undef()
    }
}

/// fn_addr face — a resolver reached without the boxed ABI has no
/// receiver-shaped calling convention to honor; loud, like the
/// builtin-method cells' native entry.
unsafe extern "C" fn iter_resolver_native_entry() -> u64 {
    unsafe extern "C" {
        fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    }
    unsafe {
        __torajs_throw_type_error(c"promise resolver called without the boxed ABI".as_ptr());
    }
    0
}

/// drop_fn — release the props bag (a user define could have grown
/// one) and this cell's block hold, then the cell itself.
unsafe extern "C" fn iter_resolver_drop(env: *mut c_void) {
    unsafe extern "C" {
        #[link_name = "__torajs_libc_free"]
        fn free(p: *mut c_void);
    }
    unsafe {
        __torajs_cycle_unbuffer(env);
        let cell = env.cast::<u8>();
        let props = *(cell.add(C_PROPS_OFF) as *const u64);
        if props != 0 {
            __torajs_value_drop_heap(props as *mut c_void);
        }
        let b = *(cell.add(C_BLOCK_OFF) as *const u64) as *mut AllBlock;
        release_block(b);
        free(env);
    }
}

/// trace_fn — the block is a plain malloc'd record, not a heap cell
/// the collector can visit; its promise edge is release_block's to
/// settle. (A resolver captured into a cycle through its block is
/// therefore invisible to the collector — the same posture every
/// promise-held edge has today.)
unsafe extern "C" fn iter_resolver_trace(
    _env: *mut c_void,
    _visit: unsafe extern "C" fn(i64, *mut c_void, *mut c_void, *mut c_void),
    _ctx: *mut c_void,
) {
}
