//! Pass 0 `declare_intrinsic` group: Array<T> runtime (M1.2).
//!
//! chunk 112 scale-up of the Pass 0 multi-sub-sibling split
//! (chunks 110/111). 11 declarations covering the Array<T> alloc /
//! grow / shrink / slice surface.
//!
//! Layout (mirrors `ARR_HDR_*` in torajs-arr):
//!   offset 0  — universal heap header (rc u32 + type_tag u16 + flags u16)
//!   offset 8  — len (u64)
//!   offset 16 — cap (u64)
//!   offset 24 — T data[cap] (uniform 8-byte slots regardless of T)
//!
//! All `arr_*` intrinsics take/return raw `Ptr` to the header;
//! grow-class intrinsics may realloc and return a NEW pointer the
//! caller must store back. Element-type-agnostic — i64 values cover
//! every primitive, Ptr values cover string / obj / nested arr.
//!
//! Subgroups:
//! - alloc / drop: `arr_alloc(initial_cap)`, `arr_drop(arr)`.
//! - push tier: `arr_push(arr, v)` (full check), `arr_push_non_deque`
//!   (12-b fast-path), `arr_push_unchecked` (M6.2 — caller pre-reserved).
//! - shift / unshift / splice: in-place memmove-based head ops.
//! - reserve / extend / slice: bulk cap mgmt + spread fast-paths.

use std::collections::HashMap;

use crate::ssa::{FuncId, Module, Type};
use crate::ssa_lower::declare_intrinsic;

pub(crate) struct ArrIds {
    pub arr_alloc: FuncId,
    pub arr_mark_kind: FuncId,
    pub arr_push: FuncId,
    pub arr_push_non_deque: FuncId,
    pub arr_shift: FuncId,
    pub arr_unshift: FuncId,
    pub arr_splice: FuncId,
    pub arr_splice_items: FuncId,
    pub arr_drop: FuncId,
    /// r500 A4' — scalar-kind array drop (no element walk, no cycle
    /// hook; props / subclass legs behind link seams).
    pub arr_drop_scalar: FuncId,
    pub arr_species_guard: FuncId,
    pub arr_len_write_guard: FuncId,
    pub arr_reserve: FuncId,
    pub arr_push_unchecked: FuncId,
    pub arr_extend_unchecked: FuncId,
    pub arr_slice: FuncId,
    pub arr_sort_cb: FuncId,
}

pub(crate) fn declare(module: &mut Module, fn_table: &mut HashMap<String, FuncId>) -> ArrIds {
    ArrIds {
        arr_alloc: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_alloc",
            &[Type::I64],
            Type::Ptr,
        ),
        // Any-dynamic-access RFC (20260704) S1 — element-kind chain
        // writer, emitted at the typed-arr→Any boxing boundary
        // (box_to_tag_value / box_to_any).
        arr_mark_kind: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_mark_kind",
            &[Type::Ptr, Type::I64],
            Type::Void,
        ),
        arr_push: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_push",
            &[Type::Ptr, Type::I64],
            Type::Ptr,
        ),
        arr_push_non_deque: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_push_non_deque",
            &[Type::Ptr, Type::I64],
            Type::Ptr,
        ),
        arr_shift: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_shift",
            &[Type::Ptr],
            Type::I64,
        ),
        arr_unshift: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_unshift",
            &[Type::Ptr, Type::I64],
            Type::Ptr,
        ),
        arr_splice: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_splice",
            &[Type::Ptr, Type::I64, Type::I64],
            Type::Ptr,
        ),
        // RFC 20260720-splice-insert knife 2 — the `...items` insert
        // form (items = stack argv of raw elem-repr slots).
        arr_splice_items: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_splice_items",
            &[Type::Ptr, Type::I64, Type::I64, Type::Ptr, Type::I64],
            Type::Ptr,
        ),
        arr_drop: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_drop",
            &[Type::Ptr],
            Type::Void,
        ),
        arr_drop_scalar: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_drop_scalar",
            &[Type::Ptr],
            Type::Void,
        ),
        arr_species_guard: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_species_guard",
            &[Type::Ptr],
            Type::I64,
        ),
        arr_len_write_guard: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_len_write_guard",
            &[Type::Ptr],
            Type::I64,
        ),
        arr_reserve: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_reserve",
            &[Type::Ptr, Type::I64],
            Type::Ptr,
        ),
        arr_push_unchecked: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_push_unchecked",
            &[Type::Ptr, Type::I64],
            Type::Void,
        ),
        arr_extend_unchecked: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_extend_unchecked",
            &[Type::Ptr, Type::Ptr],
            Type::Void,
        ),
        arr_slice: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_slice",
            &[Type::Ptr, Type::I64, Type::I64],
            Type::Ptr,
        ),
        // Perf Round 5 attack #1 (RFC 20260703-perf-arr-sort-nlogn):
        // stable O(n log n) merge sort with a user-comparator callback
        // — (arr, cmp_fn_ptr, cmp_env_ptr, mode). `mode` selects the
        // comparator's concrete C signature (elem f64 / ret f64 /
        // has-env bits); see torajs-arr/src/sort.rs.
        arr_sort_cb: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_sort_cb",
            &[Type::Ptr, Type::Ptr, Type::Ptr, Type::I64],
            Type::Void,
        ),
    }
}
