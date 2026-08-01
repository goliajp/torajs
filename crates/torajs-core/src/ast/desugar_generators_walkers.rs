//! Generator body walkers: yield-into expansion + let-lift.
//!
//! Extracted from `ast/desugar_generators.rs` when the RFC 20260713
//! blade work pushed it past the 500-line HARD limit. Both walkers
//! are `pub(crate)` re-exported through `ast::desugar_generators` so
//! `ast::desugar_generators_prep`'s existing import path
//! (`super::{expand_yield_into_in_stmt, lift_lets_in_stmt}`) keeps
//! resolving.

use super::{Ast, Expr, Stmt};

/// J.4 — recursively expand every `Stmt::YieldInto { var, type_ann,
/// value }` in `s` into the pair `[Stmt::Yield(value);
/// Stmt::LetDecl { name: var, type_ann, init: this.__sent }]`. The
/// pair is wrapped in `Stmt::Multi` so it occupies the YieldInto's
/// original slot without disturbing surrounding scope. Walks into
/// nested control-flow.
///
/// `yield_ty` is the surrounding generator's declared yield type; it
/// supplies the let's annotation when the user omitted one (so the
/// J.2.b lift picks the right field type).
pub(crate) fn expand_yield_into_in_stmt(ast: &mut Ast, s: &mut Stmt, yield_ty: &str) {
    match s {
        Stmt::YieldInto {
            var,
            type_ann,
            value,
        } => {
            let var = std::mem::take(var);
            let ty = type_ann.clone().or_else(|| Some(yield_ty.to_string()));
            let value = *value;
            let yield_stmt = Stmt::Yield(value);
            let this_id = ast.add_expr(Expr::This);
            let sent_member = ast.add_expr(Expr::Member {
                obj: this_id,
                name: "__sent".into(),
            });
            let let_stmt = Stmt::LetDecl {
                mutable: true,
                name: var,
                type_ann: ty,
                init: sent_member,
                is_var: false,
            };
            *s = Stmt::Multi(vec![yield_stmt, let_stmt]);
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            expand_yield_into_in_stmt(ast, then_branch, yield_ty);
            if let Some(eb) = else_branch.as_deref_mut() {
                expand_yield_into_in_stmt(ast, eb, yield_ty);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            expand_yield_into_in_stmt(ast, body, yield_ty);
        }
        Stmt::Labeled { body, .. } => {
            expand_yield_into_in_stmt(ast, body, yield_ty);
        }
        Stmt::For { init, body, .. } => {
            if let Some(i) = init.as_deref_mut() {
                expand_yield_into_in_stmt(ast, i, yield_ty);
            }
            expand_yield_into_in_stmt(ast, body, yield_ty);
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            for s in stmts {
                expand_yield_into_in_stmt(ast, s, yield_ty);
            }
        }
        // Switch / try cases not yet in yield scope (J.2.b).
        _ => {}
    }
}

/// Recursively replace every `let x = init` in `s` (and any nested
/// stmts) with `this.x = init`, recording each lifted `(name, type)`
/// in `lifted`. Used by `desugar_generators` so locals declared in
/// for-init / if-branches / while-bodies survive yield boundaries
/// the same way top-level lets do.
pub(crate) fn lift_lets_in_stmt(ast: &mut Ast, s: &mut Stmt, lifted: &mut Vec<(String, String)>) {
    match s {
        Stmt::LetDecl {
            name,
            type_ann,
            init,
            ..
        } => {
            let n = name.clone();
            // Knife 2a — an untyped alias of `arguments` lifts as
            // `any`: the capture rewrite turns the init into the
            // any-typed `this.arguments` field read, and the
            // historical "number" fallback would pin the lifted
            // field's type wrong (`.length on Number`).
            let t = type_ann.clone().unwrap_or_else(|| {
                if matches!(ast.get_expr(*init), Expr::Ident(nm)
                    if nm == "arguments" || nm == crate::ast::GEN_ARGV_PARAM)
                {
                    "any".into()
                } else {
                    "number".into()
                }
            });
            lifted.push((n.clone(), t));
            let this_id = ast.add_expr(Expr::This);
            let m = ast.add_expr(Expr::Member {
                obj: this_id,
                name: n,
            });
            let assign = ast.add_expr(Expr::Assign {
                target: m,
                value: *init,
            });
            *s = Stmt::Expr(assign);
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            lift_lets_in_stmt(ast, then_branch, lifted);
            if let Some(eb) = else_branch.as_deref_mut() {
                lift_lets_in_stmt(ast, eb, lifted);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            lift_lets_in_stmt(ast, body, lifted);
        }
        Stmt::Labeled { body, .. } => {
            lift_lets_in_stmt(ast, body, lifted);
        }
        Stmt::For { init, body, .. } => {
            if let Some(i) = init.as_deref_mut() {
                lift_lets_in_stmt(ast, i, lifted);
            }
            lift_lets_in_stmt(ast, body, lifted);
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            for s in stmts {
                lift_lets_in_stmt(ast, s, lifted);
            }
        }
        // Switch / try cases don't yet support yields (J.2.b scope)
        // so their inner lets stay as plain locals — no lift needed.
        _ => {}
    }
}
