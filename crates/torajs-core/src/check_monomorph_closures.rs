//! Per-specialization lifted-closure cloning — RFC
//! 20260731-mono-closure-clone 刀 1.
//!
//! A generic fn's body may construct lifted closures (arrows,
//! fn-exprs, object-literal methods). Before this pass every
//! specialization shared the ONE lifted FnDecl, so the
//! `closure_captures[fn_name]` side channel — populated per
//! construction site with the OUTER scope's local types — was
//! overwritten by whichever specialization lowered last, and the
//! shared `__env_drop_<name>` / `__env_trace_<name>` bodies walked
//! every other specialization's env with the wrong capture layout
//! (a boolean capture slot dropped as a heap pointer dereferences
//! `VALUE_TRUE = 0x7` — the test262 Iterator.concat exit-139 crash).
//!
//! The orthodox shape is the C++ / Rust generic-lambda model: a
//! closure defined inside a template body is instantiated PER
//! specialization. Each spec therefore clones every lifted closure
//! its body references (nested ones recursively), renamed with the
//! spec's `$$_...` suffix; the original decls join the lower-side
//! skip set (their only construction sites lived in the generic
//! body, which never lowers).
//!
//! `__forward_*` wrappers stay shared on purpose: they carry zero
//! captures (no layout to disagree on) and their cell is a spec-
//! mandated singleton — cloning would break `f === f` identity.

use std::collections::{HashMap, VecDeque};

use crate::ast::{Ast, Expr, ExprId, Stmt};
use crate::check::Checker;
use crate::ssa_lower_generics_monomorph::{substitute_in_ann, substitute_in_stmt};

/// Clone every lifted closure the freshly-cloned spec body references.
///
/// Runs INSIDE the caller's generic-call-site swap-capture window and
/// AFTER the spec body's own `check_stmt` — each clone is checked here
/// too, so its ExprIds land in `expr_types` and its inner generic
/// calls seed further instantiations. Bodies still reference ORIGINAL
/// closure names during checking (`closure_sig` resolves against
/// `ast.stmts`); the caller must invoke [`rewrite_closure_refs`] with
/// the returned rename map once checking is done.
///
/// Returns `(clone_decls, rename_map, rewrite_sites)`:
/// - `clone_decls` — the new FnDecls, in clone order (append AFTER the
///   spec decl so construction sites lower before closure bodies);
/// - `rename_map` — original name → clone name (feeds the rewrite and
///   the lower-side skip set);
/// - `rewrite_sites` — every `Expr::Closure` ExprId (spec body + clone
///   bodies) whose `fn_name` must be renamed.
pub(crate) fn clone_spec_closures(
    c: &mut Checker,
    owned_ast: &mut Ast,
    spec_suffix: &str,
    spec_body_id_map: &[(ExprId, ExprId)],
    subst: &[(String, String)],
) -> (Vec<Stmt>, HashMap<String, String>, Vec<ExprId>) {
    let mut rename: HashMap<String, String> = HashMap::new();
    let mut rewrite_sites: Vec<ExprId> = Vec::new();
    let mut clone_decls: Vec<Stmt> = Vec::new();
    let mut queue: VecDeque<String> = VecDeque::new();

    collect_lifted_refs(
        owned_ast,
        spec_body_id_map,
        &mut queue,
        &mut rewrite_sites,
        &rename,
    );

    while let Some(orig) = queue.pop_front() {
        if rename.contains_key(&orig) {
            continue;
        }
        let Some((params, return_type, body, is_generator, span)) =
            find_lifted_decl(owned_ast, &orig)
        else {
            // Not a lifted decl after all (e.g. resolved to something
            // without an `__env` head) — leave the reference shared.
            continue;
        };
        let clone_name = format!("{orig}{spec_suffix}");
        rename.insert(orig.clone(), clone_name.clone());

        let mut id_map: Vec<(ExprId, ExprId)> = Vec::new();
        let mut new_body: Vec<Stmt> = body
            .iter()
            .map(|s| crate::ssa_lower::deep_clone_stmt(owned_ast, &mut id_map, s))
            .collect();
        for s in new_body.iter_mut() {
            substitute_in_stmt(s, subst);
        }
        let mut new_params = params.clone();
        for p in new_params.iter_mut() {
            if let Some(ann) = &mut p.type_ann {
                *ann = substitute_in_ann(ann, subst);
            }
        }
        // Same ExprId-keyed carry the spec body clone does today
        // (destructuring group registry); the remaining table sweep is
        // the recorded L3b follow-up, kept identical for both clones.
        let cloned_groups: Vec<(ExprId, i64)> = id_map
            .iter()
            .filter_map(|&(old, new)| {
                owned_ast
                    .ary_destr_groups
                    .get(&old)
                    .map(|&limit| (new, limit))
            })
            .collect();
        owned_ast.ary_destr_groups.extend(cloned_groups);

        collect_lifted_refs(owned_ast, &id_map, &mut queue, &mut rewrite_sites, &rename);
        // Name-keyed side tables must mirror BEFORE the clone checks:
        // `closure_argv_fns` shapes the closure's checker-facing type.
        mirror_name_keyed_tables(owned_ast, &orig, &clone_name);

        let clone_decl = Stmt::FnDecl {
            name: clone_name,
            type_params: Vec::new(),
            params: new_params,
            return_type: return_type.as_ref().map(|rt| substitute_in_ann(rt, subst)),
            body: new_body,
            is_generator,
            span,
        };
        // Inference-only check, same discard discipline as the spec
        // body (these bodies were checked once already under the
        // original name — diagnostics would be duplicates).
        let saved_error_count = c.errors.len();
        c.check_stmt(owned_ast, &clone_decl);
        c.errors.truncate(saved_error_count);
        clone_decls.push(clone_decl);
    }

    (clone_decls, rename, rewrite_sites)
}

/// Point every recorded `Expr::Closure` site at its clone. Runs after
/// ALL checking (spec + clones) so `closure_sig` saw original names.
pub(crate) fn rewrite_closure_refs(
    owned_ast: &mut Ast,
    rename: &HashMap<String, String>,
    rewrite_sites: &[ExprId],
) {
    for &eid in rewrite_sites {
        if let Expr::Closure { fn_name, .. } = &mut owned_ast.exprs[eid.0 as usize]
            && let Some(clone_name) = rename.get(fn_name.as_str())
        {
            *fn_name = clone_name.clone();
        }
    }
}

/// Scan a deep-clone `id_map` for `Expr::Closure` references to
/// lifted decls: queue each name for cloning and record the CLONE-side
/// ExprId as a rewrite site. The old/new expr payloads are identical
/// at this point, so the original-side lookup answers for both.
fn collect_lifted_refs(
    ast: &Ast,
    id_map: &[(ExprId, ExprId)],
    queue: &mut VecDeque<String>,
    rewrite_sites: &mut Vec<ExprId>,
    rename: &HashMap<String, String>,
) {
    for &(old, new) in id_map {
        let Expr::Closure { fn_name, .. } = ast.get_expr(old) else {
            continue;
        };
        if fn_name.starts_with("__forward_") {
            continue;
        }
        if !is_lifted_decl(ast, fn_name) {
            continue;
        }
        rewrite_sites.push(new);
        if !rename.contains_key(fn_name) {
            queue.push_back(fn_name.clone());
        }
    }
}

/// True iff `name` resolves to a top-level FnDecl whose first param is
/// `__env` (the lifted-closure body shape).
fn is_lifted_decl(ast: &Ast, name: &str) -> bool {
    ast.stmts.iter().any(|s| {
        matches!(s, Stmt::FnDecl { name: n, params, .. }
            if n == name && params.first().is_some_and(|p| p.name == "__env"))
    })
}

type LiftedDecl = (
    Vec<crate::ast::Param>,
    Option<String>,
    Vec<Stmt>,
    bool,
    crate::lexer::Span,
);

/// The lifted FnDecl's fields, cloned out so the arena can be mutated
/// while the body is walked.
fn find_lifted_decl(ast: &Ast, name: &str) -> Option<LiftedDecl> {
    ast.stmts.iter().find_map(|s| match s {
        Stmt::FnDecl {
            name: n,
            params,
            return_type,
            body,
            is_generator,
            span,
            ..
        } if n == name && params.first().is_some_and(|p| p.name == "__env") => Some((
            params.clone(),
            return_type.clone(),
            body.clone(),
            *is_generator,
            *span,
        )),
        _ => None,
    })
}

/// Mirror every name-keyed AST side table entry from the original
/// lifted name onto the clone (no-op for tables the original isn't
/// in). These drive lower-side construction flags (`FLAG_FN_PROTO`,
/// `FLAG_FN_ASYNC`, receiver-first), the pass-2B `.name` registry,
/// and the checker's full-arguments value type.
fn mirror_name_keyed_tables(ast: &mut Ast, orig: &str, clone: &str) {
    if let Some(v) = ast.closure_self_names.get(orig).cloned() {
        ast.closure_self_names.insert(clone.to_string(), v);
    }
    if let Some(v) = ast.closure_dstr_names.get(orig).cloned() {
        ast.closure_dstr_names.insert(clone.to_string(), v);
    }
    if ast.fnexpr_recv_fns.contains(orig) {
        ast.fnexpr_recv_fns.insert(clone.to_string());
    }
    if ast.closure_argv_fns.contains(orig) {
        ast.closure_argv_fns.insert(clone.to_string());
    }
    if ast.async_fns.contains(orig) {
        ast.async_fns.insert(clone.to_string());
    }
    if ast.async_generator_fns.contains(orig) {
        ast.async_generator_fns.insert(clone.to_string());
    }
    if ast.fn_proto_fns.contains(orig) {
        ast.fn_proto_fns.insert(clone.to_string());
    }
    if ast.fn_async_value_fns.contains(orig) {
        ast.fn_async_value_fns.insert(clone.to_string());
    }
}
