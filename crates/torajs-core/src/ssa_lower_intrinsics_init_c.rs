//! Pass-0 intrinsic declarations — batch C — chunk-325 RFC of the
//! ssa_lower.rs god-file + lower_inner god-fn decomp.
//!
//! Sibling to `ssa_lower_intrinsics_init_a` (chunk-323) and
//! `ssa_lower_intrinsics_init_b` (chunk-324). Aggregates the next 6
//! sub-system declares (any_substrate / print_freeze / bigint / weak
//! / map_set / runtime_misc — 123 FuncIds total) plus the three
//! orchestration side effects that immediately follow them in the
//! original Pass 0 body:
//!
//! - the `gc` → `cycle_collect` alias insert,
//! - `ssa_lower_main_exit::declare`,
//! - `ssa_lower_process_on::declare`.
//!
//! Folding the side effects in keeps the caller's Pass 0 a single
//! line per batch. Same orchestration-only shape as init_a / init_b.

use crate::ssa::{FuncId, Module};
use std::collections::HashMap;

pub(crate) struct InitC {
    pub any_substrate: crate::ssa_lower_intrinsics_any_substrate::AnySubstrateIds,
    pub error_slots: crate::ssa_lower_intrinsics_error_slots::ErrorSlotIds,
    pub struct_expando: crate::ssa_lower_intrinsics_struct_expando::StructExpandoIds,
    pub subclass: crate::ssa_lower_intrinsics_subclass::ExoticSubclassIds,
    pub class_computed: crate::ssa_lower_intrinsics_class_computed::ClassComputedIds,
    pub private: crate::ssa_lower_intrinsics_private::PrivateIds,
    pub json_raw: crate::ssa_lower_intrinsics_json_raw::JsonRawIds,
    pub iterator: crate::ssa_lower_intrinsics_iterator::IteratorIds,
    pub print_freeze: crate::ssa_lower_intrinsics_print_freeze::PrintFreezeIds,
    pub bigint: crate::ssa_lower_intrinsics_bigint::BigIntIds,
    pub weak: crate::ssa_lower_intrinsics_weak::WeakIds,
    pub map_set: crate::ssa_lower_intrinsics_map_set::MapSetIds,
    pub set_like: crate::ssa_lower_intrinsics_set_like::SetLikeIds,
    pub runtime_misc: crate::ssa_lower_intrinsics_runtime_misc::RuntimeMiscIds,
}

pub(crate) fn declare(module: &mut Module, fn_table: &mut HashMap<String, FuncId>) -> InitC {
    let any_substrate = crate::ssa_lower_intrinsics_any_substrate::declare(module, fn_table);
    let error_slots = crate::ssa_lower_intrinsics_error_slots::declare(module, fn_table);
    let struct_expando = crate::ssa_lower_intrinsics_struct_expando::declare(module, fn_table);
    let subclass = crate::ssa_lower_intrinsics_subclass::declare(module, fn_table);
    let class_computed = crate::ssa_lower_intrinsics_class_computed::declare(module, fn_table);
    let private = crate::ssa_lower_intrinsics_private::declare(module, fn_table);
    let json_raw = crate::ssa_lower_intrinsics_json_raw::declare(module, fn_table);
    let iterator = crate::ssa_lower_intrinsics_iterator::declare(module, fn_table);
    let print_freeze = crate::ssa_lower_intrinsics_print_freeze::declare(module, fn_table);
    let bigint = crate::ssa_lower_intrinsics_bigint::declare(module, fn_table);
    let weak = crate::ssa_lower_intrinsics_weak::declare(module, fn_table);
    let map_set = crate::ssa_lower_intrinsics_map_set::declare(module, fn_table);
    let set_like = crate::ssa_lower_intrinsics_set_like::declare(module, fn_table);
    let runtime_misc = crate::ssa_lower_intrinsics_runtime_misc::declare(module, fn_table);
    // User-visible `gc()` lowers as a direct call to cycle_collect.
    // Register the alias so the existing global-fn path picks it up
    // without a new desugar.
    fn_table.insert("gc".to_string(), runtime_misc.cycle_collect);
    crate::ssa_lower_main_exit::declare(module, fn_table);
    crate::ssa_lower_process_on::declare(module, fn_table);
    InitC {
        any_substrate,
        error_slots,
        struct_expando,
        subclass,
        class_computed,
        private,
        json_raw,
        iterator,
        print_freeze,
        bigint,
        weak,
        map_set,
        set_like,
        runtime_misc,
    }
}
