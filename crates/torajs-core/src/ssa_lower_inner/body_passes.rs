//! Passes 2 / 3 / 2B / 2.5 of `lower_inner` — user-fn body lowering
//! (+ fn-name registry rows), `main` synthesis, lifted-closure body
//! lowering (reverse order), and env-drop body population, split
//! from `ssa_lower_inner.rs` (2026-07-03, fn-debt decomp). Bodies
//! verbatim; sibling of `ssa_lower_pass_3.rs` / `ssa_lower_pass_2b.rs`
//! — this module owns the Pass 2 loop and sequences the four passes.

use std::collections::HashMap;

use crate::ast::{Ast, ExprId, Stmt};
use crate::num_width::WidthTable;
use crate::ssa::{self, BakedRegexEntry, FnNameEntry, FuncId, Module, Type};
use crate::ssa_lower::Intrinsics;

#[allow(clippy::too_many_arguments)]
pub(crate) fn run(
    decl_indices: Vec<(usize, FuncId)>,
    closure_decls: Vec<(usize, FuncId)>,
    env_drop_fids: &[(String, FuncId, ssa::SigId)],
    ast: &Ast,
    module: &mut Module,
    fn_table: &HashMap<String, FuncId>,
    signatures: &HashMap<FuncId, Type>,
    fn_sig_ids: &HashMap<FuncId, ssa::SigId>,
    intrinsics: &Intrinsics,
    aliases: &HashMap<String, Type>,
    arr_layouts: &mut Vec<Type>,
    baked_regex_buf: &mut Vec<BakedRegexEntry>,
    fn_sigs: &mut Vec<(Vec<Type>, Type)>,
    struct_layouts: &mut Vec<Vec<(String, Type)>>,
    inst_memo: &mut HashMap<String, ssa::StructId>,
    generic_struct_decls: &HashMap<String, (Vec<String>, Vec<(String, String)>)>,
    closure_captures: &mut HashMap<String, Vec<(String, Type, bool)>>,
    call_retargets: &HashMap<ExprId, String>,
    may_throw: &std::collections::HashSet<String>,
    class_name_to_tag: &HashMap<String, u32>,
    anon_stamp_pool: &crate::ssa_lower_anon_stamp::AnonStampPoolCell,
    globals: &HashMap<String, Type>,
    expr_types: &HashMap<ExprId, crate::check::Type>,
    arity_pad_count: &HashMap<ExprId, usize>,
    contextual_any: &std::collections::HashSet<ExprId>,
    num_f64_slots: &WidthTable,
    promise_thunks: &crate::ssa_lower_promise_thunk::PromiseThunks,
    boxed_entries: &HashMap<FuncId, (FuncId, ssa::SigId)>,
) {
    // Pass 2: lower user FnDecl bodies. Each call returns the lowered
    // function plus any string literals interned during its body; we
    // append those into module.strings before the next call so the
    // StringId counter stays in lockstep with module.strings.len().
    for (stmt_idx, fid) in decl_indices {
        if let Stmt::FnDecl {
            name,
            params,
            return_type,
            body,
            ..
        } = &ast.stmts[stmt_idx]
        {
            let string_id_base = module.strings.len();
            let (f, new_strings) = crate::ssa_lower_fn::lower_fn(
                name,
                params,
                return_type.as_deref(),
                body,
                ast,
                fn_table,
                signatures,
                fn_sig_ids,
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
                call_retargets,
                may_throw,
                class_name_to_tag,
                anon_stamp_pool,
                globals,
                expr_types,
                arity_pad_count,
                contextual_any,
                num_f64_slots,
                promise_thunks,
                boxed_entries,
            );
            module.funcs[fid.0 as usize] = f;
            for s in new_strings {
                module.strings.push(s);
            }
            register_fn_name(module, name, params, fid);
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
        intrinsics,
        aliases,
        arr_layouts,
        baked_regex_buf,
        fn_sigs,
        struct_layouts,
        inst_memo,
        generic_struct_decls,
        closure_captures,
        call_retargets,
        may_throw,
        class_name_to_tag,
        anon_stamp_pool,
        globals,
        expr_types,
        arity_pad_count,
        contextual_any,
        num_f64_slots,
        promise_thunks,
        boxed_entries,
    );

    // Pass 2B (T-15.g.5): lower lifted-closure bodies. Delegated to
    // [`crate::ssa_lower_pass_2b::run`].
    crate::ssa_lower_pass_2b::run(
        closure_decls,
        ast,
        module,
        fn_table,
        signatures,
        fn_sig_ids,
        intrinsics,
        aliases,
        arr_layouts,
        baked_regex_buf,
        fn_sigs,
        struct_layouts,
        inst_memo,
        generic_struct_decls,
        closure_captures,
        call_retargets,
        may_throw,
        class_name_to_tag,
        anon_stamp_pool,
        globals,
        expr_types,
        arity_pad_count,
        contextual_any,
        num_f64_slots,
        promise_thunks,
        boxed_entries,
    );

    // Pass 2.5: synthesize each pre-registered env-drop fn body now
    // that closure_captures is populated. Delegated to
    // [`crate::ssa_lower_pass_2_5::populate_env_drop_bodies`].
    crate::ssa_lower_pass_2_5::populate_env_drop_bodies(
        env_drop_fids,
        closure_captures,
        intrinsics,
        module,
    );
}

/// Fn-name registry Step 2 — record the (FuncId, name, name_sid)
/// triple for the link-time __torajs_fn_name_table emit (Step 3) +
/// the runtime __torajs_fn_print_inline binary search (Step 4).
/// Skip the desugared class-method mangled forms (`__cm_<C>__<m>`,
/// `__dispatch_<m>`, `__new_<C>`) — bun reports the user-visible
/// method name on those, not the mangled name, and we get there in
/// Step 5's wire by stripping the prefix when emitting. Skip
/// generic-mono specialized names too (`<fn>__<typeargs>__<idx>`) —
/// they share the source fn's user-visible name; the entry already
/// exists for the generic form. Closure-lifted bodies
/// (`__closure_*`) are anonymous from the user's point of view;
/// runtime falls back to `[Function (anonymous)]` if no entry is
/// found.
fn register_fn_name(module: &mut Module, name: &str, params: &[crate::ast::Param], fid: FuncId) {
    if name.starts_with("__cm_")
        || name.starts_with("__dispatch_")
        || name.starts_with("__new_")
        || name.starts_with("__closure_")
        || name.contains("__mono_")
    {
        return;
    }
    // `__forward_<target>` fn-value wrappers (ast_closure_param_tag)
    // carry the user-visible TARGET name — `const t: any = topfn;
    // t.name` must answer "topfn", and the synthetic leading `__env`
    // param stays out of the arity (chunk 716).
    let visible = name.strip_prefix("__forward_").unwrap_or(name);
    // Intern the name as a Module-level string literal so the link
    // layer can resolve `__user_string_<sid>` to the rodata cstring
    // entry. encode_from_str picks Latin-1 / UTF-16 to match the
    // upstream string-literal encoding contract (TS allows non-ASCII
    // fn names).
    let lit = ssa::StringLiteral::encode_from_str(visible);
    let name_sid = ssa::StringId(module.strings.len() as u32);
    module.strings.push(lit);
    // ES-spec `Function.length` — leading params before the first
    // default / rest (§10.2.10 SetFunctionLength).
    let arity = params
        .iter()
        .filter(|p| p.name != "__env")
        .take_while(|p| p.default.is_none() && !p.is_rest)
        .count() as u32;
    module.fn_name_globals.push(FnNameEntry {
        fn_id: fid,
        name: visible.to_string(),
        name_sid,
        arity,
    });
}
