//! Pass 3 of `lower_inner` — synthesize the `main` fn from top-level
//! non-`FnDecl` statements. Skipped when the module has no top-level
//! statements. Both the synthesized fn and its newly-interned string
//! literals are appended to `module` in lockstep so the StringId
//! counter stays consistent. A module that awaits at its top level
//! gets its body as a function of its own behind the wrapper `main`
//! (`ssa_lower_fn::async_main`).
//!
//! Extracted from `lower_inner` (chunk-334 of the lower_inner RFC
//! decomp, after the Pass 0.5 / Pass 1 / Intrinsics-table /
//! module-metadata / Pass 2.5 / env-drop-setup siblings in
//! chunks 328-333). Pure mechanical move: substrate codegen invariant.

use crate::ast::{GenericStructDecls, PropKey};
use std::collections::HashMap;

use crate::ast::{Ast, ExprId, Stmt};
use crate::num_width::WidthTable;
use crate::ssa::{self, BakedRegexEntry, FuncId, Module, Type};
use crate::ssa_lower::{Intrinsics, synthesize_main};
use crate::ssa_lower_synthesize_main::main_ctx::MainEnv;

#[allow(clippy::too_many_arguments)]
pub(crate) fn run(
    ast: &Ast,
    module: &mut Module,
    fn_table: &HashMap<String, FuncId>,
    signatures: &HashMap<FuncId, Type>,
    fn_sig_ids: &HashMap<FuncId, ssa::SigId>,
    fn_dflt_lits: &crate::ssa_lower_boxed_entry::FnDfltLits,
    intrinsics: &Intrinsics,
    aliases: &HashMap<String, Type>,
    arr_layouts: &mut crate::ssa::ArrLayouts,
    baked_regex_buf: &mut Vec<BakedRegexEntry>,
    fn_sigs: &mut Vec<(Vec<Type>, Type)>,
    struct_layouts: &mut Vec<Vec<(PropKey, Type)>>,
    inst_memo: &mut HashMap<String, ssa::StructId>,
    generic_struct_decls: &GenericStructDecls,
    closure_captures: &mut HashMap<String, Vec<(String, Type, bool)>>,
    closure_capture_facts: &mut HashMap<String, crate::ssa_lower_closure_order::CaptureFacts>,
    call_retargets: &HashMap<ExprId, String>,
    may_throw: &std::collections::HashSet<String>,
    facts: &crate::ssa_lower::proofs::ProgramFactsRc,
    class_name_to_tag: &HashMap<String, u32>,
    anon_stamp_pool: &crate::ssa_lower_anon_stamp::AnonStampPoolCell,
    globals: &HashMap<String, Type>,
    expr_types: &HashMap<ExprId, crate::check::Type>,
    arity_pad_count: &HashMap<ExprId, usize>,
    contextual_any: &std::collections::HashSet<ExprId>,
    num_f64_slots: &WidthTable,
    promise_thunks: &crate::ssa_lower_promise_thunk::PromiseThunks,
    class_drop_fids: &std::collections::HashMap<u32, crate::ssa::FuncId>,
    boxed_entries: &HashMap<FuncId, (FuncId, ssa::SigId)>,
) {
    // No early return on an empty body: the link entry calls
    // `_main_user` unconditionally, so a comment-only / declaration-
    // only program still needs its (empty) main. The old
    // `top_level.is_empty()` bail was masked by the injected Error
    // hierarchy — every program used to carry its registration
    // statements — and surfaced as `UnresolvedExterns ["_main_user"]`
    // on 18 test262 cases the moment the injection-reachability gate
    // opened (rotation 497; same family as the zero-class desugar
    // bail of rotation 496).
    let top_level: Vec<&Stmt> = ast
        .stmts
        .iter()
        .filter(|s| !matches!(s, Stmt::FnDecl { .. }))
        .collect();
    let string_id_base = module.strings.len();
    // the module body goes right after the functions there are now,
    // and the wrapper `main` after it
    let body = (ast.async_ssa.main).then_some(FuncId(module.funcs.len() as u32));
    let frame_result_reprs = (module.suspend_fns.iter())
        .map(|s| (s.wrapper, s.ret_repr))
        .collect();
    let env = MainEnv {
        ast,
        fn_table,
        signatures,
        fn_sig_ids,
        fn_dflt_lits,
        intrinsics,
        aliases,
        arr_layouts,
        baked_regex_buf,
        fn_sigs,
        struct_layouts,
        inst_memo,
        generic_struct_decls,
        closure_captures,
        closure_capture_facts,
        call_retargets,
        may_throw_fns: may_throw,
        facts,
        class_name_to_tag,
        anon_stamp_pool,
        globals,
        expr_types,
        arity_pad_count,
        contextual_any,
        num_f64_slots,
        promise_thunks,
        class_drop_fids,
        boxed_entries,
    };
    let (main_fn, wrapper, new_strings) =
        synthesize_main(&top_level, env, string_id_base, body, frame_result_reprs);
    for s in new_strings {
        module.strings.push(s);
    }
    module.funcs.push(main_fn);
    if let Some(wrapper) = wrapper {
        module.funcs.push(wrapper);
    }
}
