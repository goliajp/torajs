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
use crate::ssa_lower::{Intrinsics, intern_fn_sig, synthesize_env_drop};
use crate::ssa_lower_env_trace::synthesize_env_trace;

pub(crate) fn populate_env_drop_bodies(
    env_drop_fids: &[(String, FuncId, ssa::SigId)],
    closure_captures: &HashMap<String, Vec<(String, Type, bool)>>,
    intrinsics: &Intrinsics,
    module: &mut Module,
) {
    for (closure_name, drop_fid, _drop_sig) in env_drop_fids {
        let cap_meta: Vec<(Type, bool)> = closure_captures
            .get(closure_name)
            .map(|caps| caps.iter().map(|(_, t, b)| (*t, *b)).collect())
            .unwrap_or_default();
        let f = synthesize_env_drop(&format!("__env_drop_{closure_name}"), &cap_meta, intrinsics);
        module.funcs[drop_fid.0 as usize] = f;
    }
}

/// Pass 2.5b — the paired `__env_trace_<closure>` bodies (RFC
/// 20260717 closure-env-cycle knife 2), from the same
/// `closure_captures` truth the drop bodies consume. Untraceable
/// closures get an empty body (their env stores trace_fn = 0, so
/// the collector never dispatches here); interning the visit
/// callback signature is safe at this point because
/// `module.signatures` is assigned from `fn_sigs` only at
/// `lower_inner`'s tail.
pub(crate) fn populate_env_trace_bodies(
    env_trace_fids: &[(String, FuncId)],
    closure_captures: &HashMap<String, Vec<(String, Type, bool)>>,
    fn_sigs: &mut Vec<(Vec<Type>, Type)>,
    module: &mut Module,
) {
    let visit_sig = intern_fn_sig(
        fn_sigs,
        vec![Type::I64, Type::Ptr, Type::Ptr, Type::Ptr],
        Type::Void,
    );
    for (closure_name, trace_fid) in env_trace_fids {
        let cap_meta: Vec<(Type, bool)> = closure_captures
            .get(closure_name)
            .map(|caps| caps.iter().map(|(_, t, b)| (*t, *b)).collect())
            .unwrap_or_default();
        let f = synthesize_env_trace(&format!("__env_trace_{closure_name}"), &cap_meta, visit_sig);
        module.funcs[trace_fid.0 as usize] = f;
    }
}
