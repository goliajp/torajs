//! The resolveElement / rejectElement pair a user `then` override
//! receives, for every interleaved combinator — RFC 20260810 knife
//! I3 widened this out of `combinator_iter` (which keeps the loop).
//!
//! One cell shape serves all four modes; what differs is the pair of
//! boxed entries [`entries_for`] picks:
//!
//! | mode       | resolveElement            | rejectElement            |
//! |------------|---------------------------|--------------------------|
//! | all        | park value, count out     | settle outer rejected    |
//! | allSettled | fulfilled record, count   | rejected record, count   |
//! | any        | settle outer fulfilled    | park reason, count out   |
//! | race       | settle outer fulfilled    | settle outer rejected    |
//!
//! Every entry gates on the cell's [[AlreadyCalled]] byte
//! (§27.2.4.1.2 step 1 and its per-mode mirrors) and on the block's
//! `done`; the counting entries finish the block at zero, which for
//! `any` is the AggregateError exit its `finish` arm already spells.

use core::ffi::c_void;

use crate::combinator_all_fanin::{AllBlock, release_block};
use crate::layout::{REPR_ANY, STATE_FULFILLED, STATE_REJECTED, as_promise};

unsafe extern "C" {
    #[link_name = "__torajs_libc_malloc"]
    fn malloc(n: usize) -> *mut c_void;
    fn __torajs_value_drop_heap(p: *mut c_void);
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
    fn __torajs_promise_reject(p: *mut c_void, reason: i64);
    fn __torajs_promise_resolve(p: *mut c_void, value: i64);
    /// torajs-cycle — scrub a dying cell from the root buffer.
    fn __torajs_cycle_unbuffer(p: *mut c_void);
}

/// The undefined box — no stake to account for.
unsafe fn undef() -> u64 {
    unsafe { __torajs_anyv_box_from_pair(5, 0) }
}

// ---- closure-cell layout mirror (ssa_lower closure-env / the
// `promise_with_resolvers` mint shape, lockstep) ----

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

/// A resolver's boxed-ABI entry.
pub(crate) type ResolverEntry = unsafe extern "C" fn(*mut c_void, *const u64, i64) -> u64;

/// The (resolveElement, rejectElement) pair for one mode — the table
/// in the module doc.
pub(crate) unsafe fn entries_for(mode: u8) -> (ResolverEntry, ResolverEntry) {
    use crate::combinator_all_fanin::{MODE_ALLSETTLED, MODE_ANY, MODE_RACE};
    match mode {
        m if m == MODE_ALLSETTLED => (record_fulfilled_entry, record_rejected_entry),
        m if m == MODE_ANY => (settle_fulfilled_entry, any_reject_store_entry),
        m if m == MODE_RACE => (settle_fulfilled_entry, settle_rejected_entry),
        _ => (all_resolve_elem_entry, settle_rejected_entry),
    }
}

/// One settle-function cell over the shared block — the
/// `mint_resolver` shape next crate over, captures `(block, index)`.
pub(crate) unsafe fn mint_resolver(
    b: *mut AllBlock,
    index: u64,
    entry: ResolverEntry,
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

/// §27.2.4.1.2 [[AlreadyCalled]] — true exactly once per cell.
unsafe fn claim_already(env: *mut c_void) -> bool {
    unsafe {
        let flag = env.cast::<u8>().add(C_ALREADY_OFF);
        if *flag != 0 {
            return false;
        }
        *flag = 1;
        true
    }
}

unsafe fn cell_block(env: *mut c_void) -> *mut AllBlock {
    unsafe { *(env.cast::<u8>().add(C_BLOCK_OFF) as *const u64) as *mut AllBlock }
}

unsafe fn cell_index(env: *mut c_void) -> u64 {
    unsafe { *(env.cast::<u8>().add(C_INDEX_OFF) as *const u64) }
}

unsafe fn arg0(argv: *const u64, argc: i64) -> u64 {
    if argc >= 1 {
        unsafe { *argv }
    } else {
        unsafe { undef() }
    }
}

/// Count one element out; finish the block at zero (the mode's own
/// `finish` arm — `all`/`allSettled` fulfil, `any` aggregates).
unsafe fn count_out(b: *mut AllBlock) {
    unsafe {
        (*b).remaining -= 1;
        if (*b).remaining == 0 {
            crate::combinator_fanin_slot::finish(b);
        }
    }
}

/// `all` resolveElement — park the value, count out.
unsafe extern "C" fn all_resolve_elem_entry(env: *mut c_void, argv: *const u64, argc: i64) -> u64 {
    unsafe {
        if claim_already(env) {
            let b = cell_block(env);
            if (*b).done == 0 {
                crate::combinator_fanin_slot::store_plain(b, cell_index(env), arg0(argv, argc));
                count_out(b);
            }
        }
        undef()
    }
}

/// Settle the outer with a boxed value — first settle wins. The
/// fulfilled face is `any` / `race`'s resolveElement, the rejected
/// face is the result capability's [[Reject]] every mode but
/// `allSettled` hands out.
unsafe fn settle_outer(env: *mut c_void, argv: *const u64, argc: i64, rejected: bool) -> u64 {
    unsafe {
        if claim_already(env) {
            let b = cell_block(env);
            if (*b).done == 0 {
                let v = arg0(argv, argc);
                crate::combinator_any::box_share(v);
                let rp = as_promise((*b).result);
                (*rp).value_repr = REPR_ANY;
                (*rp).value_is_heap = 1;
                (*b).done = 1;
                if rejected {
                    __torajs_promise_reject((*b).result, v as i64);
                } else {
                    __torajs_promise_resolve((*b).result, v as i64);
                }
            }
        }
        undef()
    }
}

unsafe extern "C" fn settle_fulfilled_entry(env: *mut c_void, argv: *const u64, argc: i64) -> u64 {
    unsafe { settle_outer(env, argv, argc, false) }
}

unsafe extern "C" fn settle_rejected_entry(env: *mut c_void, argv: *const u64, argc: i64) -> u64 {
    unsafe { settle_outer(env, argv, argc, true) }
}

/// `allSettled`'s pair — a `{status, value|reason}` record either
/// way, nothing short-circuits (§27.2.4.3.1).
unsafe fn record_settle(env: *mut c_void, argv: *const u64, argc: i64, state: u8) -> u64 {
    unsafe {
        if claim_already(env) {
            let b = cell_block(env);
            if (*b).done == 0 {
                let v = arg0(argv, argc);
                crate::combinator_any::box_share(v);
                let rec = crate::combinator_allsettled::alloc_settled_struct(
                    state,
                    v as i64,
                    (*b).record_tags,
                );
                crate::combinator_fanin_slot::store_slot(
                    (*b).result_arr,
                    cell_index(env),
                    rec as i64,
                );
                count_out(b);
            }
        }
        undef()
    }
}

unsafe extern "C" fn record_fulfilled_entry(env: *mut c_void, argv: *const u64, argc: i64) -> u64 {
    unsafe { record_settle(env, argv, argc, STATE_FULFILLED) }
}

unsafe extern "C" fn record_rejected_entry(env: *mut c_void, argv: *const u64, argc: i64) -> u64 {
    unsafe { record_settle(env, argv, argc, STATE_REJECTED) }
}

/// `any` rejectElement (§27.2.4.2.3) — park the reason in the
/// `errors` list, count out; zero aggregates.
unsafe extern "C" fn any_reject_store_entry(env: *mut c_void, argv: *const u64, argc: i64) -> u64 {
    unsafe {
        if claim_already(env) {
            let b = cell_block(env);
            if (*b).done == 0 {
                let v = arg0(argv, argc);
                crate::combinator_any::box_share(v);
                crate::combinator_aggregate::store_error((*b).result_arr, cell_index(env), v);
                count_out(b);
            }
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
        let b = cell_block(env);
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
