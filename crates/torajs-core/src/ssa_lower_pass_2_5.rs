//! Pass 2.5 of `lower_inner` — synthesize each pre-registered
//! `__env_drop_<closure>` fn body now that `closure_captures` is
//! populated by the Pass-2 construction sites. The drop fn frees each
//! capture slot (heap-promoted Copy boxes via `obj_drop`, non-Copy
//! values via type-specific drops) and then the env block itself.
//!
//! Extracted from `lower_inner` (chunk-332 of the lower_inner RFC
//! decomp, after the Pass 0.5 / Pass 1 / Intrinsics-table /
//! module-metadata siblings in chunks 328-331). Pure mechanical move:
//! substrate codegen invariant.

use std::collections::HashMap;

use crate::ssa::{self, FuncId, Module, Type};
use crate::ssa_lower::{Intrinsics, synthesize_env_drop};

pub(crate) fn populate_env_drop_bodies(
    env_drop_fids: &[(String, FuncId, ssa::SigId)],
    closure_captures: &HashMap<String, Vec<(Type, bool)>>,
    intrinsics: &Intrinsics,
    arr_layouts: &[Type],
    struct_layouts: &[Vec<(String, Type)>],
    module: &mut Module,
) {
    for (closure_name, drop_fid, drop_sig) in env_drop_fids {
        let cap_meta = closure_captures
            .get(closure_name)
            .cloned()
            .unwrap_or_default();
        let f = synthesize_env_drop(
            &format!("__env_drop_{closure_name}"),
            &cap_meta,
            intrinsics,
            arr_layouts,
            struct_layouts,
            *drop_sig,
        );
        module.funcs[drop_fid.0 as usize] = f;
    }
}
