//! Pass 0 `declare_intrinsic` group: queueMicrotask + Promise core +
//! fetch_sync.
//!
//! chunk 128 continuation of the Pass 0 multi-sub-sibling split
//! (chunks 110-127). 20 declarations covering the contiguous source
//! block from queueMicrotask through Promise.allSettled. Just before
//! `Object.is_f64` (separate runtime helper, stays inline).
//!
//! Subgroups (source order):
//! - **queueMicrotask** (P10.1-A1 + A1.1): closure path (cb is a
//!   Type::Closure with env+8 = fn_addr, runtime rc-inc's env +
//!   drops via `__torajs_value_drop_heap` after invoke) + simple-fn
//!   path (cb is Type::FnSig `void ()`, raw fn ptr no env). Selection
//!   at the call site by cb's static type, mirrors
//!   `promise_then_{simple,closure}` dispatch.
//! - **Promise statics** (T-15.g.1 + T-15.g.4 + T-19.f):
//!   `Promise.resolve` / `.reject` for i64-shaped values (caller
//!   packs heap ptrs / bools / f64-bitcasts), heap-value variants
//!   (`*_heap` — Promise takes ownership of one rc on inner heap
//!   value; drop dec's via `value_drop_heap`), thenable absorption
//!   (`Promise.resolve(p)` where p is itself a Promise mirrors p's
//!   (state, value) tuple inc'ing the inner's resolved-value rc).
//! - **Lifecycle**: `promise_drop`, `promise_get_value` (T-15.g.2 —
//!   `await p` desugars to `p.value` Member access at parse time).
//! - **`.then(cb)`** simple (T-15.g.3, i64→i64 MVP, cb is generic Ptr
//!   FnSig at SSA / opaque ptr at C boundary) + closure (T-15.g.5,
//!   cb is env block pointer, runtime loads fn_addr from env+8 +
//!   calls `(env, value) -> i64`; distinct dispatcher signatures
//!   `(void*, int64_t) -> int64_t` vs `(int64_t) -> int64_t`).
//! - **`.catch(cb)`** simple (T-19.k, invokes cb only on REJECTED;
//!   FULFILLED passes through) + closure (T-19.n).
//! - **`.finally(cb)`** simple + closure (T-19.k + T-19.n; cb is
//!   `() -> void` no value in/out, source state + value propagate
//!   unchanged after cb runs).
//! - **fetch_sync** (T-21 v0.6, intermixed in source order between
//!   `.finally` simple + `.catch` closure): `fetch(url)` runs a sync
//!   libcurl GET + returns a `Response*` heap struct (status @ 8,
//!   body Str* @ 16); user-side `fetch(url)` lowers as
//!   `Promise.resolve_heap(__torajs_fetch_sync(url))`.
//! - **Combinators** (T-17): `Promise.all` sync fast path (T-17.a —
//!   Array<Promise> → Promise<Array<T>>; caller responsible for
//!   input all-fulfilled at call time, full fan-in post-T-15.g.6),
//!   `Promise.race` (T-17.b — first settled mirror, all-pending →
//!   rejected), `Promise.any` (T-17.d — first fulfilled wins,
//!   all-rejected → MVP uses last seen reason vs spec
//!   AggregateError), `Promise.allSettled<number>` (T-17.c —
//!   Promise<Array<{status: string, value: number}>>).

use std::collections::HashMap;

use crate::ssa::{FuncId, Module, Type};
use crate::ssa_lower::declare_intrinsic;

pub(crate) struct PromiseIds {
    pub microtask_enqueue_closure: FuncId,
    pub microtask_enqueue_simple: FuncId,
    pub promise_alloc_fulfilled: FuncId,
    pub promise_alloc_rejected: FuncId,
    pub promise_resolve_thenable: FuncId,
    pub promise_alloc_fulfilled_heap: FuncId,
    pub promise_alloc_rejected_heap: FuncId,
    pub promise_stamp_repr: FuncId,
    pub promise_drop: FuncId,
    pub promise_get_value: FuncId,
    pub promise_then_simple: FuncId,
    pub promise_then_closure: FuncId,
    pub promise_catch_simple: FuncId,
    pub promise_finally: FuncId,
    pub fetch_sync: FuncId,
    pub promise_catch_closure: FuncId,
    pub promise_finally_closure: FuncId,
    pub promise_all_sync: FuncId,
    pub promise_race_sync: FuncId,
    pub promise_any_sync: FuncId,
    pub promise_allsettled_sync: FuncId,
}

pub(crate) fn declare(module: &mut Module, fn_table: &mut HashMap<String, FuncId>) -> PromiseIds {
    let p_ptr = &[Type::Promise, Type::Ptr][..];
    let ptr1 = &[Type::Ptr][..];
    let i641 = &[Type::I64][..];
    PromiseIds {
        microtask_enqueue_closure: declare_intrinsic(
            module,
            fn_table,
            "__torajs_queue_microtask_closure",
            ptr1,
            Type::Void,
        ),
        microtask_enqueue_simple: declare_intrinsic(
            module,
            fn_table,
            "__torajs_queue_microtask_simple",
            ptr1,
            Type::Void,
        ),
        promise_alloc_fulfilled: declare_intrinsic(
            module,
            fn_table,
            "__torajs_promise_alloc_fulfilled",
            i641,
            Type::Promise,
        ),
        promise_alloc_rejected: declare_intrinsic(
            module,
            fn_table,
            "__torajs_promise_alloc_rejected",
            i641,
            Type::Promise,
        ),
        promise_stamp_repr: declare_intrinsic(
            module,
            fn_table,
            "__torajs_promise_stamp_repr",
            &[Type::Promise, Type::I64][..],
            Type::Void,
        ),
        promise_resolve_thenable: declare_intrinsic(
            module,
            fn_table,
            "__torajs_promise_resolve_thenable",
            &[Type::Promise],
            Type::Promise,
        ),
        promise_alloc_fulfilled_heap: declare_intrinsic(
            module,
            fn_table,
            "__torajs_promise_alloc_fulfilled_heap",
            i641,
            Type::Promise,
        ),
        promise_alloc_rejected_heap: declare_intrinsic(
            module,
            fn_table,
            "__torajs_promise_alloc_rejected_heap",
            i641,
            Type::Promise,
        ),
        promise_drop: declare_intrinsic(
            module,
            fn_table,
            "__torajs_promise_drop",
            &[Type::Promise],
            Type::Void,
        ),
        promise_get_value: declare_intrinsic(
            module,
            fn_table,
            "__torajs_promise_get_value",
            &[Type::Promise],
            Type::I64,
        ),
        promise_then_simple: declare_intrinsic(
            module,
            fn_table,
            "__torajs_promise_then_simple",
            p_ptr,
            Type::Promise,
        ),
        promise_then_closure: declare_intrinsic(
            module,
            fn_table,
            "__torajs_promise_then_closure",
            p_ptr,
            Type::Promise,
        ),
        promise_catch_simple: declare_intrinsic(
            module,
            fn_table,
            "__torajs_promise_catch_simple",
            p_ptr,
            Type::Promise,
        ),
        promise_finally: declare_intrinsic(
            module,
            fn_table,
            "__torajs_promise_finally",
            p_ptr,
            Type::Promise,
        ),
        fetch_sync: declare_intrinsic(
            module,
            fn_table,
            "__torajs_fetch_sync",
            &[Type::Str],
            Type::Ptr,
        ),
        promise_catch_closure: declare_intrinsic(
            module,
            fn_table,
            "__torajs_promise_catch_closure",
            p_ptr,
            Type::Promise,
        ),
        promise_finally_closure: declare_intrinsic(
            module,
            fn_table,
            "__torajs_promise_finally_closure",
            p_ptr,
            Type::Promise,
        ),
        promise_all_sync: declare_intrinsic(
            module,
            fn_table,
            "__torajs_promise_all_sync",
            ptr1,
            Type::Promise,
        ),
        promise_race_sync: declare_intrinsic(
            module,
            fn_table,
            "__torajs_promise_race_sync",
            ptr1,
            Type::Promise,
        ),
        promise_any_sync: declare_intrinsic(
            module,
            fn_table,
            "__torajs_promise_any_sync",
            ptr1,
            Type::Promise,
        ),
        promise_allsettled_sync: declare_intrinsic(
            module,
            fn_table,
            "__torajs_promise_allsettled_sync",
            ptr1,
            Type::Promise,
        ),
    }
}
