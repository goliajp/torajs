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

use crate::ast::{Ast, Expr, ExprId, Stmt};
use crate::num_width::WidthTable;
use crate::ssa::{self, BakedRegexEntry, FnNameEntry, FuncId, Module, Type};
use crate::ssa_lower::Intrinsics;

/// Chunk 720 — ES §8.4.5 NamedEvaluation for lifted arrows: an
/// anonymous function definition bound by a `let` / `const` / `var`
/// declaration takes the binding name as its `name`. The lift pass
/// (`lift_arrow_fns`) runs per-expression with no statement context,
/// so the binding is recovered here by walking every LetDecl whose
/// init is (possibly `as`-wrapped) the lifted `Expr::Closure`.
/// Returns `__closure_N` → binding-name. Chunk 724 adds the
/// assign position (`f = () => {}`, ES §13.15.2: an Ident target
/// names the anonymous rhs), including chained `f = g = () => {}`
/// where only the innermost assignment names the arrow (the outer
/// rhs is an Assign expression, not an anonymous fn definition).
/// Object-field, default-export and deeper expression-position
/// assigns stay recorded follow-ups.
fn collect_named_eval(ast: &Ast) -> HashMap<String, String> {
    fn init_closure_name(ast: &Ast, eid: ExprId) -> Option<&str> {
        match ast.get_expr(eid) {
            Expr::Closure { fn_name, .. } if fn_name.starts_with("__closure_") => Some(fn_name),
            Expr::As { expr, .. } => init_closure_name(ast, *expr),
            _ => None,
        }
    }
    fn walk_assign(ast: &Ast, eid: ExprId, map: &mut HashMap<String, String>) {
        if let Expr::Assign { target, value } = ast.get_expr(eid) {
            if let (Expr::Ident(n), Some(cn)) =
                (ast.get_expr(*target), init_closure_name(ast, *value))
            {
                map.insert(cn.to_string(), n.clone());
            }
            walk_assign(ast, *value, map);
        }
    }
    fn walk(ast: &Ast, s: &Stmt, map: &mut HashMap<String, String>) {
        match s {
            Stmt::LetDecl { name, init, .. } => {
                if let Some(cn) = init_closure_name(ast, *init) {
                    map.insert(cn.to_string(), name.clone());
                }
            }
            Stmt::Expr(eid) => walk_assign(ast, *eid, map),
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                walk(ast, then_branch, map);
                if let Some(e) = else_branch {
                    walk(ast, e, map);
                }
            }
            Stmt::While { body, .. }
            | Stmt::DoWhile { body, .. }
            | Stmt::ForOfSplitIter { body, .. }
            | Stmt::ForOf { body, .. } => walk(ast, body, map),
            Stmt::For { init, body, .. } => {
                if let Some(i) = init {
                    walk(ast, i, map);
                }
                walk(ast, body, map);
            }
            Stmt::Switch { cases, default, .. } => {
                for c in cases {
                    for cs in &c.body {
                        walk(ast, cs, map);
                    }
                }
                if let Some(d) = default {
                    for ds in d {
                        walk(ast, ds, map);
                    }
                }
            }
            Stmt::Try {
                body,
                catch_body,
                finally_body,
                ..
            } => {
                for bs in body.iter().chain(catch_body.iter()) {
                    walk(ast, bs, map);
                }
                if let Some(f) = finally_body {
                    for fs in f {
                        walk(ast, fs, map);
                    }
                }
            }
            Stmt::Block(v) | Stmt::Multi(v) => {
                for bs in v {
                    walk(ast, bs, map);
                }
            }
            Stmt::FnDecl { body, .. } => {
                for bs in body {
                    walk(ast, bs, map);
                }
            }
            Stmt::ExportDecl { inner, .. } => {
                if let Some(i) = inner {
                    walk(ast, i, map);
                }
            }
            _ => {}
        }
    }
    let mut map = HashMap::new();
    for s in &ast.stmts {
        walk(ast, s, &mut map);
    }
    map
}

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
    let named_eval = collect_named_eval(ast);
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
                contextual_any,
                num_f64_slots,
                promise_thunks,
                boxed_entries,
            );
            module.funcs[fid.0 as usize] = f;
            for s in new_strings {
                module.strings.push(s);
            }
            // Chunk 720 — NamedEvaluation registry row: a let-bound
            // arrow answers its binding name / arity through the
            // fn-addr registry (`.name` / `.length` / fn-print),
            // mirroring the Pass 2 fn-decl rows in `body_passes`.
            // The link layer sorts the table by fn_addr, so the
            // late push order is immaterial.
            if let Some(binding) = named_eval.get(name) {
                let lit = ssa::StringLiteral::encode_from_str(binding);
                let name_sid = ssa::StringId(module.strings.len() as u32);
                module.strings.push(lit);
                // ES-spec `Function.length` — leading params before
                // the first default / rest (§10.2.10), synthetic
                // `__env` excluded (chunk 716 contract).
                let arity = params
                    .iter()
                    .filter(|p| p.name != "__env")
                    .take_while(|p| p.default.is_none() && !p.is_rest)
                    .count() as u32;
                module.fn_name_globals.push(FnNameEntry {
                    fn_id: fid,
                    name: binding.clone(),
                    name_sid,
                    arity,
                });
            }
        }
    }
}
