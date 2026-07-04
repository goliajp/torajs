//! Pass 2B (T-15.g.5) of `lower_inner` — lower lifted-closure bodies.
//! Deferred until after Pass 3 main-synth so top-level construction
//! sites (`let cb = function(v) { ... }` at module scope) have
//! populated `closure_captures`. Closures still lower in reverse
//! append order among themselves so an outer closure's body (which
//! constructs the inner closure) runs before the inner closure's
//! body.
//!
//! Extracted from `lower_inner` (chunk-335 RFC continuation, after
//! the Pass 2.5 / Pass 3 siblings in chunks 332 + 334). Pure
//! mechanical move: substrate codegen invariant.

use std::collections::HashMap;

use crate::ast::{Ast, ExprId, Stmt};
use crate::num_width::WidthTable;
use crate::ssa::{self, BakedRegexEntry, FuncId, Module, Type};
use crate::ssa_lower::Intrinsics;

#[allow(clippy::too_many_arguments)]
pub(crate) fn run(
    closure_decls: Vec<(usize, FuncId)>,
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
    closure_captures: &mut HashMap<String, Vec<(Type, bool)>>,
    call_retargets: &HashMap<ExprId, String>,
    may_throw: &std::collections::HashSet<String>,
    class_name_to_tag: &HashMap<String, u32>,
    anon_stamp_pool: &crate::ssa_lower_anon_stamp::AnonStampPoolCell,
    globals: &HashMap<String, Type>,
    expr_types: &HashMap<ExprId, crate::check::Type>,
    arity_pad_count: &HashMap<ExprId, usize>,
    num_f64_slots: &WidthTable,
    promise_thunks: &crate::ssa_lower_promise_thunk::PromiseThunks,
    boxed_entries: &HashMap<FuncId, (FuncId, ssa::SigId)>,
) {
    for (stmt_idx, fid) in closure_decls {
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
                num_f64_slots,
                promise_thunks,
                boxed_entries,
            );
            module.funcs[fid.0 as usize] = f;
            for s in new_strings {
                module.strings.push(s);
            }
        }
    }
}
