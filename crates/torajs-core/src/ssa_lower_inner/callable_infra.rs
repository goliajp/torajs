//! The callable infrastructure `lower_inner` sets up between Pass 1
//! and the body passes: per-closure env-drop / env-trace fns, the
//! per-class release fns, the promise-callback ABI thunks, and the
//! return-type snapshot of every callable. All of it must exist
//! before the fn list freezes at that snapshot.

use std::collections::HashMap;

use crate::ast::{Ast, PropKey};
use crate::ssa::{self, FuncId, Module, Type};

pub(super) struct CallableInfra {
    pub env_drop_fids: Vec<(String, FuncId, ssa::SigId)>,
    pub env_drop_trivial_fid: (FuncId, ssa::SigId),
    pub env_trace_fids: Vec<(String, FuncId)>,
    pub class_drops: Vec<crate::ssa_lower_emit_drop_value::class_drop_setup::ClassDropFn>,
    pub promise_thunks: crate::ssa_lower_promise_thunk::PromiseThunks,
    pub signatures: HashMap<FuncId, Type>,
}

/// Env-drop fn infrastructure + per-class release fns + promise-
/// callback ABI thunks + the callables return-type snapshot. Split
/// 2026-07-03 (fn-debt decomp); bodies verbatim.
#[allow(clippy::too_many_arguments)]
pub(super) fn setup_callable_infra(
    ast: &Ast,
    module: &mut Module,
    fn_table: &mut HashMap<String, FuncId>,
    fn_sigs: &mut Vec<(Vec<Type>, Type)>,
    fn_sig_ids: &mut HashMap<FuncId, ssa::SigId>,
    init_a: &crate::ssa_lower_intrinsics_init_a::InitA,
    num_f64_slots: &crate::num_width::WidthTable,
    aliases: &HashMap<String, Type>,
    struct_layouts: &[Vec<(PropKey, Type)>],
) -> CallableInfra {
    // Env-drop fn infrastructure (per-closure pre-allocate + trivial
    // wrapper) — see [`crate::ssa_lower_env_drop_setup`].
    let env_drop_setup =
        crate::ssa_lower_env_drop_setup::run(ast, module, fn_table, fn_sigs, fn_sig_ids, init_a);
    let env_drop_fids = env_drop_setup.env_drop_fids;
    let env_drop_trivial_fid = env_drop_setup.env_drop_trivial_fid;
    let env_trace_fids = env_drop_setup.env_trace_fids;

    // Per-class release fns `__drop_<C>` — pre-registered here, bodies
    // in `body_passes` once every layout is final.
    let class_drops = crate::ssa_lower_emit_drop_value::class_drop_setup::run(
        ast,
        module,
        fn_sigs,
        fn_sig_ids,
        aliases,
        struct_layouts,
    );

    // ②.6b — promise callback ABI thunks (bits-adapters for f64-faced
    // `.then` / `.catch` handlers). Synthesized here because the fn
    // list freezes at the signatures snapshot below; modules without
    // a promise chain synthesize nothing.
    let promise_thunks = crate::ssa_lower_promise_thunk::synthesize_promise_thunks(
        ast,
        num_f64_slots,
        module,
        fn_table,
        fn_sigs,
        fn_sig_ids,
        init_a.obj_capture.obj_drop_sized,
        init_a.obj_capture.value_drop_heap,
        init_a.obj_capture.cycle_unbuffer,
    );

    // Snapshot every callable's return type — used inside lower_fn to type
    // call-site results correctly.
    let signatures: HashMap<FuncId, Type> = module
        .funcs
        .iter()
        .enumerate()
        .map(|(i, f)| (FuncId(i as u32), f.ret))
        .collect();
    CallableInfra {
        env_drop_fids,
        env_drop_trivial_fid,
        env_trace_fids,
        class_drops,
        promise_thunks,
        signatures,
    }
}
