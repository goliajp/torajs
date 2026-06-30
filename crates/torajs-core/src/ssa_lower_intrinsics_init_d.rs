//! Pass-0 intrinsic declarations — batch D — chunk-326 RFC of the
//! ssa_lower.rs god-file + lower_inner god-fn decomp.
//!
//! Sibling to init_a (chunk-323) / init_b (chunk-324) / init_c
//! (chunk-325). Aggregates the final 8 sub-system declares
//! (promise / substr / substr_trim_into / arr_str_etc / str_extra
//! / math / json_misc / throw — 170 FuncIds total) so `lower_inner`'s
//! Pass 0 collapses 180 LOC of destructure boilerplate to a single
//! line and Pass 0 is fully drained from the caller.
//!
//! `substr_trim_into::declare_all` returns a tuple instead of a
//! struct; we re-flatten the tuple into a `SubstrTrimIds` named
//! struct local to this module for uniform `init_d.<group>.<field>`
//! access at the call site.

use crate::ssa::{FuncId, Module};
use std::collections::HashMap;

pub(crate) struct SubstrTrimIds {
    pub substr_trim: FuncId,
    pub substr_trim_start: FuncId,
    pub substr_trim_end: FuncId,
    pub substr_trim_into: FuncId,
}

pub(crate) struct InitD {
    pub promise: crate::ssa_lower_intrinsics_promise::PromiseIds,
    pub substr: crate::ssa_lower_intrinsics_substr::SubstrIds,
    pub substr_trim: SubstrTrimIds,
    pub arr_str_etc: crate::ssa_lower_intrinsics_arr_str_etc::ArrStrEtcIds,
    pub str_extra: crate::ssa_lower_intrinsics_str_extra::StrExtraIds,
    pub math: crate::ssa_lower_intrinsics_math::MathIds,
    pub json_misc: crate::ssa_lower_intrinsics_json_misc::JsonMiscIds,
    pub throw: crate::ssa_lower_intrinsics_throw::ThrowIds,
}

pub(crate) fn declare(module: &mut Module, fn_table: &mut HashMap<String, FuncId>) -> InitD {
    let promise = crate::ssa_lower_intrinsics_promise::declare(module, fn_table);
    let substr = crate::ssa_lower_intrinsics_substr::declare(module, fn_table);
    let (substr_trim, substr_trim_start, substr_trim_end, substr_trim_into) =
        crate::ssa_lower_substr_trim_into::declare_all(module, fn_table);
    let arr_str_etc = crate::ssa_lower_intrinsics_arr_str_etc::declare(module, fn_table);
    let str_extra = crate::ssa_lower_intrinsics_str_extra::declare(module, fn_table);
    let math = crate::ssa_lower_intrinsics_math::declare(module, fn_table);
    let json_misc = crate::ssa_lower_intrinsics_json_misc::declare(module, fn_table);
    let throw = crate::ssa_lower_intrinsics_throw::declare(module, fn_table);
    InitD {
        promise,
        substr,
        substr_trim: SubstrTrimIds {
            substr_trim,
            substr_trim_start,
            substr_trim_end,
            substr_trim_into,
        },
        arr_str_etc,
        str_extra,
        math,
        json_misc,
        throw,
    }
}
