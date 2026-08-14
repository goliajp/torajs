//! Pass 0 `declare_intrinsic` group: Map / Set / MapIter / ArrIter
//! strong-ref runtime (P6.1 + P6.4b + P6.4c-C3).
//!
//! chunk 126 continuation of the Pass 0 multi-sub-sibling split
//! (chunks 110-125). 29 declarations covering the entire
//! `Map<K,V>` / `Set<T>` surface plus the keys/values/entries
//! iterator factories + step + drop for both Map sources and
//! Array<Any> sources.
//!
//! ABI notes (mirrors the per-decl comments that previously lived
//! inline):
//! - **Map K/V** is tagged-Any with SameValueZero key equality.
//!   `set` / `has` / `delete` take the key as an unboxed
//!   `(tag, payload)` i64-pair so typed-tier callers (e.g.
//!   `Map<string, T>`) avoid a temp Any-box alloc; the
//!   `box_to_tag_value` helper produces the pair from a Type::Any
//!   source. `get` returns into a `(tag, payload)` out pair via
//!   stack-slot pointers.
//! - **MapIter** (P6.4b): the iter holds a strong ref to the source
//!   Map; `map_iter_step` advances + fills the
//!   `(value_tag, value_payload)` pair so the SSA side can rebuild
//!   the `IteratorResult<any>` struct each call. `set_entries`
//!   factory wraps a Set source the same way.
//! - **ArrIter** (P6.4c-C3): same shape as MapIter, distinct
//!   intrinsics for `Array<Any>` source layout.

use std::collections::HashMap;

use crate::ssa::{FuncId, Module, Type};
use crate::ssa_lower::declare_intrinsic;

pub(crate) struct MapSetIds {
    pub map_create: FuncId,
    pub set_create: FuncId,
    pub set_is_subset_of: FuncId,
    pub set_is_superset_of: FuncId,
    pub set_is_disjoint_from: FuncId,
    pub set_union: FuncId,
    pub set_intersection: FuncId,
    pub set_difference: FuncId,
    pub set_symmetric_difference: FuncId,
    pub map_clone: FuncId,
    pub map_set: FuncId,
    pub map_get: FuncId,
    pub map_get_or_insert: FuncId,
    pub map_get_or_insert_computed: FuncId,
    pub map_has: FuncId,
    pub map_delete: FuncId,
    pub map_clear: FuncId,
    pub map_size: FuncId,
    pub map_drop: FuncId,
    pub map_iter_next: FuncId,
    pub map_iter_create_keys: FuncId,
    pub map_iter_create_values: FuncId,
    pub map_iter_create_entries: FuncId,
    pub map_iter_create_set_entries: FuncId,
    pub arr_iter_create_keys: FuncId,
    pub arr_iter_create_values: FuncId,
    pub arr_iter_create_entries: FuncId,
    pub arr_iter_step: FuncId,
    pub arr_iter_drop: FuncId,
    pub map_iter_step: FuncId,
    pub map_iter_drop: FuncId,
}

// CARVE-OUT: dispatch table — 26 back-to-back `declare_intrinsic` calls
// filling a single `MapSetIds` struct literal (data-driven; each entry
// declares one runtime FuncId + records it in `fn_table`). Refactor
// blocked by MapSetIds being non-composable — splitting into sub-structs
// or a macro shape helper would either change the `intrinsics.<field>`
// consumer surface (architectural churn across every ssa_lower map_set
// read site) or trade line count for a `decl!(name, params, ret)` macro
// that saves ≤ 3 lines per entry against the added macro-hygiene cost.
// Same rationale as `ssa_lower_intrinsics_table.rs::build`.
pub(crate) fn declare(module: &mut Module, fn_table: &mut HashMap<String, FuncId>) -> MapSetIds {
    let set_pair = &[Type::Set, Type::Set][..];
    MapSetIds {
        map_create: declare_intrinsic(module, fn_table, "__torajs_map_create", &[], Type::Map),
        set_create: declare_intrinsic(module, fn_table, "__torajs_set_create", &[], Type::Set),
        set_is_subset_of: declare_intrinsic(
            module,
            fn_table,
            "__torajs_set_is_subset_of",
            set_pair,
            Type::I64,
        ),
        set_is_superset_of: declare_intrinsic(
            module,
            fn_table,
            "__torajs_set_is_superset_of",
            set_pair,
            Type::I64,
        ),
        set_is_disjoint_from: declare_intrinsic(
            module,
            fn_table,
            "__torajs_set_is_disjoint_from",
            set_pair,
            Type::I64,
        ),
        set_union: declare_intrinsic(module, fn_table, "__torajs_set_union", set_pair, Type::Set),
        set_intersection: declare_intrinsic(
            module,
            fn_table,
            "__torajs_set_intersection",
            set_pair,
            Type::Set,
        ),
        set_difference: declare_intrinsic(
            module,
            fn_table,
            "__torajs_set_difference",
            set_pair,
            Type::Set,
        ),
        set_symmetric_difference: declare_intrinsic(
            module,
            fn_table,
            "__torajs_set_symmetric_difference",
            set_pair,
            Type::Set,
        ),
        map_clone: declare_intrinsic(
            module,
            fn_table,
            "__torajs_map_clone",
            &[Type::Map],
            Type::Map,
        ),
        map_set: declare_intrinsic(
            module,
            fn_table,
            "__torajs_map_set",
            &[Type::Map, Type::I64, Type::I64, Type::I64, Type::I64],
            Type::Void,
        ),
        map_get: declare_intrinsic(
            module,
            fn_table,
            "__torajs_map_get",
            &[Type::Map, Type::I64, Type::I64, Type::Ptr, Type::Ptr],
            Type::Void,
        ),
        map_get_or_insert: declare_intrinsic(
            module,
            fn_table,
            "__torajs_map_get_or_insert",
            &[
                Type::Map,
                Type::I64,
                Type::I64,
                Type::I64,
                Type::I64,
                Type::Ptr,
                Type::Ptr,
            ],
            Type::Void,
        ),
        map_get_or_insert_computed: declare_intrinsic(
            module,
            fn_table,
            "__torajs_map_get_or_insert_computed",
            &[Type::Map, Type::I64, Type::I64, Type::Any],
            Type::Any,
        ),
        map_has: declare_intrinsic(
            module,
            fn_table,
            "__torajs_map_has",
            &[Type::Map, Type::I64, Type::I64],
            Type::I64,
        ),
        map_delete: declare_intrinsic(
            module,
            fn_table,
            "__torajs_map_delete",
            &[Type::Map, Type::I64, Type::I64],
            Type::I64,
        ),
        map_clear: declare_intrinsic(
            module,
            fn_table,
            "__torajs_map_clear",
            &[Type::Map],
            Type::Void,
        ),
        map_size: declare_intrinsic(
            module,
            fn_table,
            "__torajs_map_size",
            &[Type::Map],
            Type::I64,
        ),
        map_drop: declare_intrinsic(
            module,
            fn_table,
            "__torajs_map_drop",
            &[Type::Map],
            Type::Void,
        ),
        map_iter_next: declare_intrinsic(
            module,
            fn_table,
            "__torajs_map_iter_next",
            &[
                Type::Map,
                Type::Ptr,
                Type::Ptr,
                Type::Ptr,
                Type::Ptr,
                Type::Ptr,
            ],
            Type::I64,
        ),
        map_iter_create_keys: declare_intrinsic(
            module,
            fn_table,
            "__torajs_map_iter_create_keys",
            &[Type::Map],
            Type::MapIter,
        ),
        map_iter_create_values: declare_intrinsic(
            module,
            fn_table,
            "__torajs_map_iter_create_values",
            &[Type::Map],
            Type::MapIter,
        ),
        map_iter_create_entries: declare_intrinsic(
            module,
            fn_table,
            "__torajs_map_iter_create_entries",
            &[Type::Map],
            Type::MapIter,
        ),
        map_iter_create_set_entries: declare_intrinsic(
            module,
            fn_table,
            "__torajs_map_iter_create_set_entries",
            &[Type::Set],
            Type::MapIter,
        ),
        arr_iter_create_keys: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_iter_create_keys",
            &[Type::Ptr],
            Type::ArrIter,
        ),
        arr_iter_create_values: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_iter_create_values",
            &[Type::Ptr],
            Type::ArrIter,
        ),
        arr_iter_create_entries: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_iter_create_entries",
            &[Type::Ptr],
            Type::ArrIter,
        ),
        arr_iter_step: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_iter_step",
            &[Type::ArrIter, Type::Ptr, Type::Ptr],
            Type::I64,
        ),
        arr_iter_drop: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_iter_drop",
            &[Type::ArrIter],
            Type::Void,
        ),
        map_iter_step: declare_intrinsic(
            module,
            fn_table,
            "__torajs_map_iter_step",
            &[Type::MapIter, Type::Ptr, Type::Ptr],
            Type::I64,
        ),
        map_iter_drop: declare_intrinsic(
            module,
            fn_table,
            "__torajs_map_iter_drop",
            &[Type::MapIter],
            Type::Void,
        ),
    }
}
