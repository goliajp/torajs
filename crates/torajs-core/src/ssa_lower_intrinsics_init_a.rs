//! Pass-0 intrinsic declarations — batch A — chunk-323 RFC of the
//! ssa_lower.rs god-file + lower_inner god-fn decomp.
//!
//! Aggregates the first 6 sub-system declares (print_str / obj_capture
//! / arr / str_a / num / str_b) into a single `InitA` holder so
//! `lower_inner`'s Pass 0 collapses 108 LOC of destructure boilerplate
//! to a single call site. Each sub-system already lives in its own
//! `ssa_lower_intrinsics_<name>` sibling — this module is a pure
//! orchestration layer with no logic of its own.
//!
//! The caller (`lower_inner`) accesses the per-sub-Ids fields via
//! `init_a.<group>.<field>` when building the final `Intrinsics`
//! struct literal.

use crate::ssa::{FuncId, Module};
use std::collections::HashMap;

pub(crate) struct InitA {
    pub print_str: crate::ssa_lower_intrinsics_print_str::PrintStrIds,
    pub obj_capture: crate::ssa_lower_intrinsics_obj_capture::ObjCaptureIds,
    pub arr: crate::ssa_lower_intrinsics_arr::ArrIds,
    pub str_a: crate::ssa_lower_intrinsics_str_a::StrAIds,
    pub num: crate::ssa_lower_intrinsics_num::NumIds,
    pub str_b: crate::ssa_lower_intrinsics_str_b::StrBIds,
}

pub(crate) fn declare(module: &mut Module, fn_table: &mut HashMap<String, FuncId>) -> InitA {
    InitA {
        print_str: crate::ssa_lower_intrinsics_print_str::declare(module, fn_table),
        obj_capture: crate::ssa_lower_intrinsics_obj_capture::declare(module, fn_table),
        arr: crate::ssa_lower_intrinsics_arr::declare(module, fn_table),
        str_a: crate::ssa_lower_intrinsics_str_a::declare(module, fn_table),
        num: crate::ssa_lower_intrinsics_num::declare(module, fn_table),
        str_b: crate::ssa_lower_intrinsics_str_b::declare(module, fn_table),
    }
}
