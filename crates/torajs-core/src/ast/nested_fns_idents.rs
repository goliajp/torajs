//! Ident-rewrite walk for the nested-fn hoisting pass — split from
//! `nested_fns` (labeled break/continue work) to keep that file under
//! the size limit. Rewrites references to a lifted nested fn's original
//! name into its mangled top-level name across a statement subtree.

mod expr;

pub(super) use expr::rewrite_idents_in_expr;

use super::{Ast, Stmt};
use std::collections::HashMap;

pub(super) fn rewrite_idents_in_body(
    ast: &mut Ast,
    body: &mut [Stmt],
    renames: &HashMap<String, String>,
    arrows: bool,
) {
    for s in body.iter_mut() {
        rewrite_idents_in_stmt(ast, s, renames, arrows);
    }
}

pub(super) fn rewrite_idents_in_stmt(
    ast: &mut Ast,
    s: &mut Stmt,
    renames: &HashMap<String, String>,
    arrows: bool,
) {
    match s {
        Stmt::Expr(eid) => rewrite_idents_in_expr(ast, *eid, renames, arrows),
        Stmt::LetDecl { init, .. } => rewrite_idents_in_expr(ast, *init, renames, arrows),
        Stmt::Return(Some(eid)) => rewrite_idents_in_expr(ast, *eid, renames, arrows),
        Stmt::Throw(eid) => rewrite_idents_in_expr(ast, *eid, renames, arrows),
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            rewrite_idents_in_expr(ast, *cond, renames, arrows);
            rewrite_idents_in_stmt(ast, then_branch, renames, arrows);
            if let Some(eb) = else_branch.as_deref_mut() {
                rewrite_idents_in_stmt(ast, eb, renames, arrows);
            }
        }
        Stmt::Labeled { body, .. } => rewrite_idents_in_stmt(ast, body, renames, arrows),
        Stmt::While { cond, body } | Stmt::DoWhile { body, cond } => {
            rewrite_idents_in_expr(ast, *cond, renames, arrows);
            rewrite_idents_in_stmt(ast, body, renames, arrows);
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            if let Some(i) = init.as_deref_mut() {
                rewrite_idents_in_stmt(ast, i, renames, arrows);
            }
            if let Some(c) = cond {
                rewrite_idents_in_expr(ast, *c, renames, arrows);
            }
            if let Some(st) = step {
                rewrite_idents_in_expr(ast, *st, renames, arrows);
            }
            rewrite_idents_in_stmt(ast, body, renames, arrows);
        }
        Stmt::Block(b) | Stmt::Multi(b) => {
            for s2 in b.iter_mut() {
                rewrite_idents_in_stmt(ast, s2, renames, arrows);
            }
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            for s2 in body.iter_mut() {
                rewrite_idents_in_stmt(ast, s2, renames, arrows);
            }
            for s2 in catch_body.iter_mut() {
                rewrite_idents_in_stmt(ast, s2, renames, arrows);
            }
            if let Some(fb) = finally_body {
                for s2 in fb.iter_mut() {
                    rewrite_idents_in_stmt(ast, s2, renames, arrows);
                }
            }
        }
        Stmt::Switch {
            scrutinee,
            cases,
            default,
            ..
        } => {
            rewrite_idents_in_expr(ast, *scrutinee, renames, arrows);
            for case in cases.iter_mut() {
                rewrite_idents_in_expr(ast, case.value, renames, arrows);
                for s2 in case.body.iter_mut() {
                    rewrite_idents_in_stmt(ast, s2, renames, arrows);
                }
            }
            if let Some(dflt) = default {
                for s2 in dflt.iter_mut() {
                    rewrite_idents_in_stmt(ast, s2, renames, arrows);
                }
            }
        }
        Stmt::ForOfSplitIter {
            parent, sep, body, ..
        } => {
            rewrite_idents_in_expr(ast, *parent, renames, arrows);
            rewrite_idents_in_expr(ast, *sep, renames, arrows);
            rewrite_idents_in_stmt(ast, body, renames, arrows);
        }
        Stmt::ForOf {
            elem_expr, body, ..
        } => {
            rewrite_idents_in_expr(ast, *elem_expr, renames, arrows);
            rewrite_idents_in_stmt(ast, body, renames, arrows);
        }
        Stmt::Yield(eid) | Stmt::YieldInto { value: eid, .. } => {
            rewrite_idents_in_expr(ast, *eid, renames, arrows);
        }
        _ => {}
    }
}

/// True when an arrow rebinds `name` itself — as a parameter, or as a
/// declaration at the top of its body. Such a reference belongs to the
/// arrow, not to the enclosing scope whose declaration is being
/// renamed, so it must keep the original name.
///
/// Deliberately shallow: a rebinding nested deeper than the arrow's
/// own statement list shadows only within that inner block, where a
/// reference to the outer name cannot appear anyway.
///
/// Shared with `desugar_generators_rewrite`, which descends into arrow
/// bodies for the same reason and needs the same exclusion.
pub(super) fn arrow_rebinds(params: &[super::Param], body: &[Stmt], name: &str) -> bool {
    params.iter().any(|p| p.name == name)
        || body.iter().any(|s| match s {
            Stmt::LetDecl { name: n, .. } | Stmt::FnDecl { name: n, .. } => n == name,
            _ => false,
        })
}

/// 421-04 K.2 closure — rewrite a renamed import decl's free
/// self-references. `import { fact as f1 }` injects the decl spelled
/// `f1`, but its recursive call still reads `fact`, which is not in
/// the importer's scope (the long-recorded K.2 corner). Same lexical
/// walk the nested-fn lift uses for its mangled names; the resolver
/// lives outside `ast`, so this fronts it. FnDecl-only — a LetDecl
/// initializer never references its own binding (TDZ), and the other
/// decl kinds don't rename through this lane.
pub(crate) fn rename_fn_self_refs(ast: &mut Ast, s: &mut Stmt, orig: &str, new_name: &str) {
    if let Stmt::FnDecl { body, .. } = s {
        let renames = HashMap::from([(orig.to_string(), new_name.to_string())]);
        rewrite_idents_in_body(ast, body, &renames, true);
    }
}
