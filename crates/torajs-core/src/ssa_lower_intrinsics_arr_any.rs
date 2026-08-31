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
//!   possibly-realloc'd ptr). `arr_typed_set_grow` (typed-tier
//!   OOB grow-as-holes — RFC 20260721-typed-grow-on-write).
//! - Indexed read: `arr_get_any_tag` / `_value` (P1.4 — bounds-
//!   checking, returns ANY_UNDEF=5 / value=0 for OOB per ES
//!   §10.4.2.1).

use std::collections::HashMap;

use crate::ssa::{FuncId, Module, Type};
use crate::ssa_lower::declare_intrinsic;

pub(crate) struct ArrAnyIds {
    pub arr_alloc_any: FuncId,
    pub arr_push_any: FuncId,
    /// Elision hole marking — see `__torajs_arr_mark_last_hole`.
    pub arr_mark_last_hole: FuncId,
    /// §23.1.3.32 per-element toLocaleString join.
    pub arr_any_to_locale_string: FuncId,
    /// Typed-lane toLocaleString patch gate (RFC 20260721 刀 11 G13).
    pub arr_typed_to_locale_string: FuncId,
    /// Hole-aware §23.1.3.17/.20 search kernels (HasProperty gate).
    pub arr_any_index_of: FuncId,
    pub arr_any_last_index_of: FuncId,
    pub arr_unshift_any: FuncId,
    pub arr_fill_any: FuncId,
    pub arr_extend_any: FuncId,
    /// Rotation 546 — one Any-typed concat argument: runtime
    /// §23.1.3.1 is-array test (spread vs append) inside the kernel.
    pub arr_concat_any_arg: FuncId,
    pub arr_any_slice: FuncId,
    pub arr_any_to_reversed: FuncId,
    pub arr_has_index: FuncId,
    pub arr_set_any: FuncId,
    pub arr_set_any_grow: FuncId,
    pub arr_typed_set_grow: FuncId,
    /// Chunk B — static typed lane post-store hole revive.
    pub arr_index_revive_idx: FuncId,
    pub arr_index_check_store: FuncId,
    pub arr_null_check: FuncId,
    pub arr_get_any_tag: FuncId,
    pub arr_get_any_value: FuncId,
    pub arr_get_any_boxed: FuncId,
    /// Rotation 468 — the owned read: inline views materialize, else rc-inc.
    pub arr_get_any_owned: FuncId,
    pub arr_any_pop: FuncId,
    pub arr_any_shift: FuncId,
    /// RFC 20260708-closure-argv-face — `__torajs_arr_any_push(arr,
    /// argv, argc, recv_slot)` bulk append (borrow-in, incs stored
    /// heap cells); the `__torajs_arguments` materializer's second
    /// half.
    pub arr_any_push: FuncId,
    /// §10.4.4 arguments-materialization stamp — sets
    /// FLAG_ARR_ARGUMENTS on the fresh `__torajs_arguments` cell so
    /// the keyed `"length"` readers answer the arguments-exotic
    /// attributes (configurable true, deletable).
    pub arr_mark_arguments: FuncId,
    /// §10.4.4.6 step 21 — the strict `arguments.callee` read
    /// (%ThrowTypeError% getter; answers undefined with the pending
    /// TypeError).
    pub arguments_callee: FuncId,
}

pub(crate) fn declare(module: &mut Module, fn_table: &mut HashMap<String, FuncId>) -> ArrAnyIds {
    // Single-line declaration table (chunk 457 idiom — macro_rules
    // locals are hygienic, so `module` / `fn_table` resolve at the
    // definition site).
    macro_rules! decl {
        ($name:literal, [$($param:ident),*], $ret:ident) => {
            declare_intrinsic(module, fn_table, $name, &[$(Type::$param),*], Type::$ret)
        };
    }
    let arr_alloc_any = decl!("__torajs_arr_alloc_any", [I64], Ptr);
    // P0.10 `__torajs_arr_alloc_any_filled` — fn_table-only
    // registration. Its FuncId isn't held by the caller (resolved at
    // ssa_lower time via desugar Ident lookup), so it stays
    // anonymous; keep it inside `declare` so the registration order
    // mirrors the pre-extract inline source.
    let _ = decl!("__torajs_arr_alloc_any_filled", [I64], Ptr);
    // §23.1.2.1 step 4.b — f64 entry for non-integer-provable Number
    // operands; keeps the fractional/NaN/Infinity bits the i64
    // coercion erases so the ToUint32(len) != len RangeError fires.
    let _ = decl!("__torajs_arr_alloc_any_filled_f64", [F64], Ptr);
    // §23.1.1.1 step 4 — `new Array(x)` with an Any operand is a
    // runtime type test (a Number is a length, anything else is the
    // single element), so the whole dispatch is one kernel.
    let _ = decl!("__torajs_arr_new_from_any", [Any], Ptr);
    // Chunk 698 `__torajs_arr_any_to_typed` — fn_table-only (the
    // let-decl assign-boundary conversion resolves it by name).
    let _ = decl!("__torajs_arr_any_to_typed", [Ptr, I64], Ptr);
    ArrAnyIds {
        arr_alloc_any,
        arr_any_push: decl!("__torajs_arr_any_push", [Ptr, Ptr, I64, Ptr], I64),
        arr_mark_arguments: decl!("__torajs_arr_mark_arguments", [Ptr], Void),
        arguments_callee: decl!("__torajs_arguments_callee", [], Any),
        arr_push_any: decl!("__torajs_arr_push_any", [Ptr, I64, I64], Ptr),
        arr_mark_last_hole: decl!("__torajs_arr_mark_last_hole", [Ptr], Void),
        arr_any_to_locale_string: decl!("__torajs_arr_any_to_locale_string", [Ptr], Str),
        arr_typed_to_locale_string: decl!("__torajs_arr_typed_to_locale_string", [Ptr, I64], Str),
        arr_any_index_of: decl!("__torajs_arr_any_index_of", [Ptr, Any, I64], I64),
        arr_any_last_index_of: decl!("__torajs_arr_any_last_index_of", [Ptr, Any, I64], I64),
        arr_unshift_any: decl!("__torajs_arr_unshift_any", [Ptr, I64, I64], Ptr),
        arr_fill_any: decl!("__torajs_arr_fill_any", [Ptr, I64, I64, I64, I64], Ptr),
        arr_extend_any: decl!("__torajs_arr_extend_any", [Ptr, Ptr], Ptr),
        arr_concat_any_arg: decl!("__torajs_arr_concat_any_arg", [Ptr, Any], Ptr),
        // Flag-aware slice (concat's fresh-copy seed): FLAG_ARR_ANY
        // propagates onto the fresh header + per-slot NaN-box-safe
        // rc_inc — unlike raw arr_slice, whose product is flag-blind
        // (the drop walker then never decs the elements).
        arr_any_slice: decl!("__torajs_arr_any_slice", [Ptr, I64, I64], Ptr),
        // 刀 13b — §23.1.3.33 high→low [[Get]]-order reverse copy
        // (getter side effects observed by later lower-index reads).
        arr_any_to_reversed: decl!("__torajs_arr_any_to_reversed", [Ptr], Ptr),
        // 刀 13d — §7.3.11 numeric-key `in` on an Array receiver
        // (hole shadows + prototype digit keys; clean in-bounds
        // stays one flag test).
        arr_has_index: decl!("__torajs_arr_has_index", [Ptr, I64], I64),
        arr_set_any: decl!("__torajs_arr_set_any", [Ptr, I64, I64, I64], Void),
        arr_set_any_grow: decl!("__torajs_arr_set_any_grow", [Ptr, I64, I64, I64], Ptr),
        arr_typed_set_grow: decl!("__torajs_arr_typed_set_grow", [Ptr, I64, I64], Void),
        arr_index_revive_idx: decl!("__torajs_arr_index_revive_idx", [Ptr, I64], Void),
        arr_index_check_store: decl!("__torajs_arr_index_check_store", [Ptr, I64], Void),
        arr_null_check: decl!("__torajs_arr_null_check", [Ptr], Void),
        arr_get_any_tag: decl!("__torajs_arr_get_any_tag", [Ptr, I64], I64),
        arr_get_any_value: decl!("__torajs_arr_get_any_value", [Ptr, I64], I64),
        arr_get_any_boxed: decl!("__torajs_arr_get_any_boxed", [Ptr, I64], Any),
        arr_get_any_owned: decl!("__torajs_arr_get_any_owned", [Ptr, I64], Any),
        // Chunk 628 — kind-aware pop/shift for static Arr<Any>
        // receivers (boxed result, empty → undefined, typed-behind-
        // any blocks rebox per elem kind; the any-method-call RFC's
        // runtime pair).
        arr_any_pop: decl!("__torajs_arr_any_pop", [Ptr], Any),
        arr_any_shift: decl!("__torajs_arr_any_shift", [Ptr], Any),
    }
}
