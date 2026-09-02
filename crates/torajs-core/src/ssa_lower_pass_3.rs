//! Pass 3 of `lower_inner` — synthesize the `main` fn from top-level
//! non-`FnDecl` statements. Skipped when the module has no top-level
//! statements. Both the synthesized fn and its newly-interned string
//! literals are appended to `module` in lockstep so the StringId
//! counter stays consistent.
//!
//! Extracted from `lower_inner` (chunk-334 of the lower_inner RFC
//! decomp, after the Pass 0.5 / Pass 1 / Intrinsics-table /
//! module-metadata / Pass 2.5 / env-drop-setup siblings in
//! chunks 328-333). Pure mechanical move: substrate codegen invariant.

use std::collections::HashMap;

use crate::ast::{Ast, ExprId, Stmt};
use crate::num_width::WidthTable;
use crate::ssa::{self, BakedRegexEntry, FuncId, Module, Type};
use crate::ssa_lower::{Intrinsics, synthesize_main};

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
    arr_layouts: &mut Vec<Type>,
    baked_regex_buf: &mut Vec<BakedRegexEntry>,
    fn_sigs: &mut Vec<(Vec<Type>, Type)>,
    struct_layouts: &mut Vec<Vec<(String, Type)>>,
    inst_memo: &mut HashMap<String, ssa::StructId>,
    generic_struct_decls: &HashMap<String, (Vec<String>, Vec<(String, String)>)>,
    closure_captures: &mut HashMap<String, Vec<(String, Type, bool)>>,
    closure_variadic_captures: &mut HashMap<String, Vec<String>>,
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
    let (main_fn, new_strings) = synthesize_main(
        &top_level,
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
        closure_variadic_captures,
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
    for s in new_strings {
        module.strings.push(s);
    }
    module.funcs.push(main_fn);
}
