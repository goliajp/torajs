//! Pass 0 `declare_intrinsic` group: Array<Any> tagged-slot runtime
//! (T-10.b / P0.10 / P1.4 / P5.6 / bug-327 C3).
//!
//! chunk 120 continuation of the Pass 0 multi-sub-sibling split
//! (chunks 110-119). 10 declarations (one anonymous = registered in
//! fn_table only — `__torajs_arr_alloc_any_filled` resolved at SSA
//! lower time via fn_table lookup, no caller takes its FuncId).
//!
//! `arr_*_any` uses a 16-byte-stride array (vs 8 for regular
//! `Array<T>`), each slot a `{tag, value}` AnyValue pair. Drop walks
//! via `__torajs_arr_drop_any` (called by the regular Array drop path
//! when `ARR_FLAG_ANY` is set; not yet wired — T-10.d).
//!
//! - Alloc:
//!   - `arr_alloc_any(cap) -> Ptr` (T-10.b).
//!   - `__torajs_arr_alloc_any_filled(n) -> Ptr` (P0.10 — `new
//!     Array(n)`: alloc + fill ANY_NULL; anonymous fn_table-only
//!     registration so AST-level desugar's `Expr::Call { callee =
//!     Ident("__torajs_arr_alloc_any_filled") }` resolves).
//! - Mutators: `arr_push_any` / `arr_fill_any(arr, tag, val, start,
//!   end)` (L3b) / `arr_extend_any(dst, src)` (P5.6 — bumps src's
//!   heap children's refcount).
//! - Indexed write: `arr_set_any(arr, idx, tag, val)` (P0.10 — drops
//!   old slot if ANY_HEAP; caller must rc-bump heap values).
//!   `arr_set_any_grow` (bug-327 C3 — growable variant returning
//!   possibly-realloc'd ptr). `arr_oob_write_reject` (typed-tier
//!   OOB).
//! - Indexed read: `arr_get_any_tag` / `_value` (P1.4 — bounds-
//!   checking, returns ANY_UNDEF=5 / value=0 for OOB per ES
//!   §10.4.2.1).

use std::collections::HashMap;

use crate::ssa::{FuncId, Module, Type};
use crate::ssa_lower::declare_intrinsic;

pub(crate) struct ArrAnyIds {
    pub arr_alloc_any: FuncId,
    pub arr_push_any: FuncId,
    pub arr_unshift_any: FuncId,
    pub arr_fill_any: FuncId,
    pub arr_extend_any: FuncId,
    pub arr_any_slice: FuncId,
    pub arr_set_any: FuncId,
    pub arr_set_any_grow: FuncId,
    pub arr_oob_write_reject: FuncId,
    pub arr_null_check: FuncId,
    pub arr_get_any_tag: FuncId,
    pub arr_get_any_value: FuncId,
    pub arr_get_any_boxed: FuncId,
    pub arr_any_pop: FuncId,
    pub arr_any_shift: FuncId,
    /// RFC 20260708-closure-argv-face — `__torajs_arr_any_push(arr,
    /// argv, argc, recv_slot)` bulk append (borrow-in, incs stored
    /// heap cells); the `__torajs_arguments` materializer's second
    /// half.
    pub arr_any_push: FuncId,
}

pub(crate) fn declare(module: &mut Module, fn_table: &mut HashMap<String, FuncId>) -> ArrAnyIds {
    let arr_alloc_any = declare_intrinsic(
        module,
        fn_table,
        "__torajs_arr_alloc_any",
        &[Type::I64],
        Type::Ptr,
    );
    // P0.10 `__torajs_arr_alloc_any_filled` — fn_table-only
    // registration. Its FuncId isn't held by the caller (resolved at
    // ssa_lower time via desugar Ident lookup), so it stays
    // anonymous; keep it inside `declare` so the registration order
    // mirrors the pre-extract inline source.
    let _ = declare_intrinsic(
        module,
        fn_table,
        "__torajs_arr_alloc_any_filled",
        &[Type::I64],
        Type::Ptr,
    );
    ArrAnyIds {
        arr_alloc_any,
        arr_any_push: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_any_push",
            &[Type::Ptr, Type::Ptr, Type::I64, Type::Ptr],
            Type::I64,
        ),
        arr_push_any: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_push_any",
            &[Type::Ptr, Type::I64, Type::I64],
            Type::Ptr,
        ),
        arr_unshift_any: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_unshift_any",
            &[Type::Ptr, Type::I64, Type::I64],
            Type::Ptr,
        ),
        arr_fill_any: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_fill_any",
            &[Type::Ptr, Type::I64, Type::I64, Type::I64, Type::I64],
            Type::Ptr,
        ),
        arr_extend_any: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_extend_any",
            &[Type::Ptr, Type::Ptr],
            Type::Ptr,
        ),
        // Flag-aware slice (concat's fresh-copy seed): FLAG_ARR_ANY
        // propagates onto the fresh header + per-slot NaN-box-safe
        // rc_inc — unlike raw arr_slice, whose product is flag-blind
        // (the drop walker then never decs the elements).
        arr_any_slice: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_any_slice",
            &[Type::Ptr, Type::I64, Type::I64],
            Type::Ptr,
        ),
        arr_set_any: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_set_any",
            &[Type::Ptr, Type::I64, Type::I64, Type::I64],
            Type::Void,
        ),
        arr_set_any_grow: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_set_any_grow",
            &[Type::Ptr, Type::I64, Type::I64, Type::I64],
            Type::Ptr,
        ),
        arr_oob_write_reject: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_oob_write_reject",
            &[Type::I64],
            Type::Void,
        ),
        arr_null_check: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_null_check",
            &[Type::Ptr],
            Type::Void,
        ),
        arr_get_any_tag: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_get_any_tag",
            &[Type::Ptr, Type::I64],
            Type::I64,
        ),
        arr_get_any_value: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_get_any_value",
            &[Type::Ptr, Type::I64],
            Type::I64,
        ),
        arr_get_any_boxed: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_get_any_boxed",
            &[Type::Ptr, Type::I64],
            Type::Any,
        ),
        // Chunk 628 — kind-aware pop/shift for static Arr<Any>
        // receivers (boxed result, empty → undefined, typed-behind-
        // any blocks rebox per elem kind; the any-method-call RFC's
        // runtime pair).
        arr_any_pop: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_any_pop",
            &[Type::Ptr],
            Type::Any,
        ),
        arr_any_shift: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_any_shift",
            &[Type::Ptr],
            Type::Any,
        ),
    }
}
