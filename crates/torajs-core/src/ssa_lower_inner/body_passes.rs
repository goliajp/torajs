//! Passes 2 / 3 / 2B / 2.5 of `lower_inner` — user-fn body lowering
//! (+ fn-name registry rows), `main` synthesis, lifted-closure body
//! lowering (reverse order), and env-drop body population, split
//! from `ssa_lower_inner.rs` (2026-07-03, fn-debt decomp). Bodies
//! verbatim; sibling of `ssa_lower_pass_3.rs` / `ssa_lower_pass_2b.rs`
//! — this module owns the Pass 2 loop and sequences the four passes.

use std::collections::HashMap;

use super::body_passes_fn_name::register_fn_name;
use crate::ast::{Ast, ExprId, GenericStructDecls, PropKey, Stmt};
use crate::num_width::WidthTable;
use crate::ssa::{self, BakedRegexEntry, FuncId, Module, Type};
use crate::ssa_lower::Intrinsics;
use crate::ssa_lower_fn::async_setup;

#[allow(clippy::too_many_arguments)]
pub(crate) fn run(
    suspend: &crate::ssa_lower_fn::async_setup::SuspendSetup,
    decl_indices: Vec<(usize, FuncId)>,
    closure_decls: Vec<(usize, FuncId)>,
    env_drop_fids: &[(String, FuncId, ssa::SigId)],
    env_trace_fids: &[(String, FuncId)],
    class_drops: &[crate::ssa_lower_emit_drop_value::class_drop_setup::ClassDropFn],
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
    scans: crate::ssa_lower_ctx_state::ModuleScans,
) -> crate::ssa_lower_emit_drop_value::class_drop_fill::DropHooks {
    // Pass 2: user FnDecl bodies; each body's interned strings go to
    // module.strings at once, so StringIds stay in step with its length.
    for (stmt_idx, fid) in decl_indices {
        if let Stmt::FnDecl {
            name,
            params,
            return_type,
            body,
            span,
            ..
        } = &ast.stmts[stmt_idx]
        {
            let string_id_base = module.strings.len();
            let (f, new_strings, holds) = crate::ssa_lower_fn::lower_fn(
                name,
                params,
                return_type.as_deref(),
                body,
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
                string_id_base,
                closure_captures,
                closure_capture_facts,
                call_retargets,
                may_throw,
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
                &scans,
            );
            facts.catch.mark_lowered(name);
            async_setup::install(module, fid, (f, holds), suspend, intrinsics, arr_layouts);
            module.strings.extend(new_strings);
            register_fn_name(module, name, params, fid, ast, *span, boxed_entries);
        }
    }

    // Pass 3: synthesize `main` from top-level non-FnDecl statements.
    // Delegated to [`crate::ssa_lower_pass_3::run`].
    crate::ssa_lower_pass_3::run(
        ast,
        module,
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
        may_throw,
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
    );

    // Pass 2B (T-15.g.5): lower lifted-closure bodies. Delegated to
    // [`crate::ssa_lower_pass_2b::run`].
    crate::ssa_lower_pass_2b::run(
        suspend,
        closure_decls,
        ast,
        module,
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
        may_throw,
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
        &scans,
    );

    // Pass 2.5: synthesize each pre-registered env-drop fn body now
    // that closure_captures is populated. Delegated to
    // [`crate::ssa_lower_pass_2_5::populate_env_drop_bodies`].
    crate::ssa_lower_pass_2_5::populate_env_drop_bodies(
        env_drop_fids,
        closure_captures,
        &ast.closure_self_names,
        intrinsics,
        module,
    );

    // Pass 2.5b: the paired env-trace fn bodies (RFC 20260717
    // closure-env-cycle knife 2) from the same closure_captures
    // truth — [`crate::ssa_lower_pass_2_5::populate_env_trace_bodies`].
    crate::ssa_lower_pass_2_5::populate_env_trace_bodies(
        env_trace_fids,
        closure_captures,
        fn_sigs,
        module,
    );

    // Pass 2.5c: the per-class release fn bodies, over the settled layouts.
    let hooks = crate::ssa_lower_emit_drop_value::class_drop_fill::populate_class_drop_bodies(
        class_drops,
        class_name_to_tag,
        aliases,
        struct_layouts,
        arr_layouts,
        anon_stamp_pool,
        intrinsics,
        module,
    );
    crate::ssa_lower_emit_drop_value::class_drop_reentry::route(class_drops, fn_sigs, module);
    hooks
}
