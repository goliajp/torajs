//! Pass-0 intrinsic declarations — batch B — chunk-324 RFC of the
//! ssa_lower.rs god-file + lower_inner god-fn decomp.
//!
//! Sibling to `ssa_lower_intrinsics_init_a` (chunk-323). Aggregates
//! the next 6 sub-system declares (regex / date / fs / process /
//! arr_any / object — 121 FuncIds total) so `lower_inner`'s Pass 0
//! batch B collapses 148 LOC of destructure boilerplate to a single
//! call site. Same orchestration-only shape as init_a; the caller
//! reads `init_b.<group>.<field>` directly in the `Intrinsics { ... }`
//! literal.

use crate::ssa::{FuncId, Module};
use std::collections::HashMap;

pub(crate) struct InitB {
    pub regex: crate::ssa_lower_intrinsics_regex::RegexIds,
    pub date: crate::ssa_lower_intrinsics_date::DateIds,
    pub fs: crate::ssa_lower_intrinsics_fs::FsIds,
    pub process: crate::ssa_lower_intrinsics_process::ProcessIds,
    pub arr_any: crate::ssa_lower_intrinsics_arr_any::ArrAnyIds,
    pub object: crate::ssa_lower_intrinsics_object::ObjectIds,
}

pub(crate) fn declare(module: &mut Module, fn_table: &mut HashMap<String, FuncId>) -> InitB {
    InitB {
        regex: crate::ssa_lower_intrinsics_regex::declare(module, fn_table),
        date: crate::ssa_lower_intrinsics_date::declare(module, fn_table),
        fs: crate::ssa_lower_intrinsics_fs::declare(module, fn_table),
        process: crate::ssa_lower_intrinsics_process::declare(module, fn_table),
        arr_any: crate::ssa_lower_intrinsics_arr_any::declare(module, fn_table),
        object: crate::ssa_lower_intrinsics_object::declare(module, fn_table),
    }
}
