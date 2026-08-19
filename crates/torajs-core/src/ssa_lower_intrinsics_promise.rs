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
    /// §27.2.4.7 step 2 through the any lane — probes the boxed
    /// value for a %Promise% cell (pass-through) before falling back
    /// to the fulfilled_heap + REPR_ANY pair.
    pub promise_resolve_any: FuncId,
    /// §27.2.4 static-slot patch consult pair (rotation 448) — the
    /// call-site gate over the ctor cell's expando dict, and the
    /// typed lane's Any→Promise return contract on a patched call.
    pub promise_ctor_patched: FuncId,
    pub promise_patched_result: FuncId,
    pub promise_stamp_repr: FuncId,
    pub promise_drop: FuncId,
    pub promise_get_value: FuncId,
    pub promise_get_value_as: FuncId,
    pub anyv_await: FuncId,
    pub promise_then_simple: FuncId,
    pub promise_then_passthrough: FuncId,
    pub promise_then2: FuncId,
    pub promise_then_closure: FuncId,
    pub promise_catch_simple: FuncId,
    pub promise_finally: FuncId,
    pub fetch_sync: FuncId,
    pub promise_catch_closure: FuncId,
    pub promise_finally_closure: FuncId,
    pub promise_all_sync: FuncId,
    pub promise_with_resolvers: FuncId,
    pub promise_race_sync: FuncId,
    pub promise_any_sync: FuncId,
    pub promise_allsettled_sync: FuncId,
    /// RFC 20260730 knife A — combinators over a statically
    /// non-iterable (any-boxed) argument; §27.2.4 GetIterator
    /// failure answers a rejected promise at runtime.
    pub promise_all_dyn: FuncId,
    pub promise_race_dyn: FuncId,
    pub promise_any_dyn: FuncId,
    pub promise_allsettled_dyn: FuncId,
    /// await-dictionary — keyed combinators over an OBJECT argument;
    /// fulfill with a null-prototype object keyed like the input.
    pub promise_all_keyed_dyn: FuncId,
    pub promise_allsettled_keyed_dyn: FuncId,
    /// `Array.fromAsync(items)` sync-source MVP — the array-like
    /// step protocol collects, promise elements unwrap (§2.1.1
    /// step 5.e award), result promise holds an `Array<Any>`.
    pub array_from_async_dyn: FuncId,
    /// The mapped form — per element: await, `mapfn(value, k)`,
    /// await the mapped result (§2.1.1 steps 5.e-5.j interleaving).
    pub array_from_async_map_dyn: FuncId,
}

// CARVE-OUT: dispatch table — back-to-back `declare_intrinsic` calls
// filling the PromiseIds struct literal (same family as the
// `intrinsics_map_set` / `intrinsics_print_freeze` declare tables;
// registered as a carve-out candidate in rotation 255's audit, stamp
// applied on first touch per the ledger's instruction).
pub(crate) fn declare(module: &mut Module, fn_table: &mut HashMap<String, FuncId>) -> PromiseIds {
    // knife 3 — then/catch entries carry the callback-return repr
    // (RFC 20260720-anylane-promise-methods). `finally` joined them
    // in rotation 301: §27.2.5.3 declares `onFinally` as `() => any`
    // and WAITS on a thenable it returns, so the kernel has to know
    // whether the return register holds anything and what form.
    let p_ptr_repr = &[Type::Promise, Type::Ptr, Type::I64][..];
    let ptr1 = &[Type::Ptr][..];
    // One trailing word the call site alone can supply, since SSA's
    // `Type::Promise` erases the inner T: for `all` the element form
    // its result array must hold, for `allSettled` the class tag its
    // `{status, value}` records must carry.
    let ptr_repr = &[Type::Ptr, Type::I64][..];
    // `allSettled` needs a second: the class tags its records carry AND
    // the form their value slot holds.
    let ptr_repr2 = &[Type::Ptr, Type::I64, Type::I64][..];
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
        promise_resolve_any: declare_intrinsic(
            module,
            fn_table,
            "__torajs_promise_resolve_any",
            i641,
            Type::Promise,
        ),
        promise_ctor_patched: declare_intrinsic(
            module,
            fn_table,
            "__torajs_promise_ctor_patched",
            i641,
            Type::I64,
        ),
        promise_patched_result: declare_intrinsic(
            module,
            fn_table,
            "__torajs_promise_patched_result",
            &[Type::Any, Type::I64],
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
        // RFC 20260727 blade 3 — the same read, told which typed lane
        // the awaiting site will cast into so a cell settled from an
        // `any` gets unboxed instead of reinterpreted.
        promise_get_value_as: declare_intrinsic(
            module,
            fn_table,
            "__torajs_promise_get_value_as",
            &[Type::Promise, Type::I64],
            Type::I64,
        ),
        // rotation 233 — `await <any>` by-VALUE dispatch: a heap
        // Promise cell unwraps (boxed per its repr stamp, +1 stake),
        // everything else passes through identity.
        anyv_await: declare_intrinsic(
            module,
            fn_table,
            "__torajs_anyv_await",
            &[Type::Any],
            Type::Any,
        ),
        promise_then_simple: declare_intrinsic(
            module,
            fn_table,
            "__torajs_promise_then_simple",
            p_ptr_repr,
            Type::Promise,
        ),
        promise_then_passthrough: declare_intrinsic(
            module,
            fn_table,
            "__torajs_promise_then_passthrough",
            &[Type::Promise],
            Type::Promise,
        ),
        promise_then2: declare_intrinsic(
            module,
            fn_table,
            "__torajs_promise_then2",
            &[Type::Promise, Type::Ptr, Type::I64, Type::Ptr, Type::I64][..],
            Type::Promise,
        ),
        promise_then_closure: declare_intrinsic(
            module,
            fn_table,
            "__torajs_promise_then_closure",
            p_ptr_repr,
            Type::Promise,
        ),
        promise_catch_simple: declare_intrinsic(
            module,
            fn_table,
            "__torajs_promise_catch_simple",
            p_ptr_repr,
            Type::Promise,
        ),
        promise_finally: declare_intrinsic(
            module,
            fn_table,
            "__torajs_promise_finally",
            p_ptr_repr,
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
            p_ptr_repr,
            Type::Promise,
        ),
        promise_finally_closure: declare_intrinsic(
            module,
            fn_table,
            "__torajs_promise_finally_closure",
            p_ptr_repr,
            Type::Promise,
        ),
        promise_all_sync: declare_intrinsic(
            module,
            fn_table,
            "__torajs_promise_all_sync",
            ptr_repr,
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
            ptr_repr2,
            Type::Promise,
        ),
        promise_all_dyn: declare_intrinsic(
            module,
            fn_table,
            "__torajs_promise_all_dyn",
            &[Type::Any],
            Type::Promise,
        ),
        promise_race_dyn: declare_intrinsic(
            module,
            fn_table,
            "__torajs_promise_race_dyn",
            &[Type::Any],
            Type::Promise,
        ),
        promise_any_dyn: declare_intrinsic(
            module,
            fn_table,
            "__torajs_promise_any_dyn",
            &[Type::Any],
            Type::Promise,
        ),
        promise_allsettled_dyn: declare_intrinsic(
            module,
            fn_table,
            "__torajs_promise_allsettled_dyn",
            &[Type::Any],
            Type::Promise,
        ),
        promise_all_keyed_dyn: declare_intrinsic(
            module,
            fn_table,
            "__torajs_promise_all_keyed_dyn",
            &[Type::Any],
            Type::Promise,
        ),
        promise_allsettled_keyed_dyn: declare_intrinsic(
            module,
            fn_table,
            "__torajs_promise_allsettled_keyed_dyn",
            &[Type::Any],
            Type::Promise,
        ),
        array_from_async_dyn: declare_intrinsic(
            module,
            fn_table,
            "__torajs_array_from_async_dyn",
            &[Type::Any],
            Type::Promise,
        ),
        array_from_async_map_dyn: declare_intrinsic(
            module,
            fn_table,
            "__torajs_array_from_async_map_dyn",
            &[Type::Any, Type::Any, Type::Any],
            Type::Promise,
        ),
        promise_with_resolvers: declare_intrinsic(
            module,
            fn_table,
            "__torajs_promise_with_resolvers",
            &[],
            Type::Any,
        ),
    }
}
