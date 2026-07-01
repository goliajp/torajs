//! Uninit-let init-splice pass — chunk 359, extracted from ast.rs.
//!
//! Pub entry `desugar_uninit_let` promotes the first matching
//! same-scope follow-up assignment into a bare `let x;`'s init slot
//! (rewriting `let x; x = EXPR;` → `let x = EXPR;`). Any `let`
//! without a matching follow-up keeps the `Expr::Uninit` sentinel so
//! the typechecker can report a clean "let declared but never
//! assigned" error. Private helper `rewrite_uninit_in_stmts` walks
//! each declaring scope (FnDecl body, block, if/while/for/try body).

use super::{Ast, Expr, ExprId, Stmt};

/// `let x;` (the `var x;` shape after the test262 runner's `var → let`
/// rewrite) parses to `Stmt::LetDecl { init: Expr::Uninit }`. This
/// pass walks each declaring scope, finds the first
/// `Stmt::Expr(Assign { Ident(x), value })` after the let, splices
/// `value` into the let's init, and removes the assignment. Anything
/// that doesn't have a matching follow-up assignment keeps the
/// `Uninit` sentinel; the typechecker reports it with a clear "let
/// declared but never assigned" message — better than the previous
/// `expected `=`, got Semi` parse error.
///
/// Limitations of the search:
///   - same scope only — won't promote an inner-block assignment to
///     the outer let's init, since that would change scope semantics
///   - first matching assignment wins — chains like
///     `let x; if (...) x = 1; else x = "two";` don't unify; only the
///     first branch's value lifts in, the second stays an assign and
///     the regular type checker handles the agreement check
///   - top-level vs fn-body scopes are walked uniformly; nested
///     control-flow children don't bubble assignments across their
///     boundary (we only splice within the same `Vec<Stmt>`)
pub fn desugar_uninit_let(ast: &mut Ast) {
    rewrite_uninit_in_stmts(&mut ast.stmts, &ast.exprs.clone());
    // FnDecl bodies live inside `ast.stmts` already; the recursive
    // walk handles them when it descends into Stmt::FnDecl variants.
}

fn rewrite_uninit_in_stmts(stmts: &mut Vec<Stmt>, exprs: &[Expr]) {
    let mut i = 0;
    while i < stmts.len() {
        // Recurse into nested scopes first so each scope's lets see
        // their own follow-up assignments.
        match &mut stmts[i] {
            Stmt::FnDecl { body, .. } => {
                rewrite_uninit_in_stmts(body, exprs);
            }
            Stmt::Block(inner) | Stmt::Multi(inner) => {
                rewrite_uninit_in_stmts(inner, exprs);
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                if let Stmt::Block(b) | Stmt::Multi(b) = then_branch.as_mut() {
                    rewrite_uninit_in_stmts(b, exprs);
                }
                if let Some(eb) = else_branch
                    && let Stmt::Block(b) | Stmt::Multi(b) = eb.as_mut()
                {
                    rewrite_uninit_in_stmts(b, exprs);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                if let Stmt::Block(b) | Stmt::Multi(b) = body.as_mut() {
                    rewrite_uninit_in_stmts(b, exprs);
                }
            }
            Stmt::For { body, .. } => {
                if let Stmt::Block(b) | Stmt::Multi(b) = body.as_mut() {
                    rewrite_uninit_in_stmts(b, exprs);
                }
            }
            Stmt::Try {
                body,
                catch_body,
                finally_body,
                ..
            } => {
                rewrite_uninit_in_stmts(body, exprs);
                rewrite_uninit_in_stmts(catch_body, exprs);
                if let Some(fb) = finally_body {
                    rewrite_uninit_in_stmts(fb, exprs);
                }
            }
            _ => {}
        }
        // Now, if this stmt is an Uninit let, scan forward for the
        // first matching `name = EXPR;` and splice.
        let (name, init_eid) = match &stmts[i] {
            Stmt::LetDecl { name, init, .. } => (name.clone(), *init),
            _ => {
                i += 1;
                continue;
            }
        };
        let is_uninit = matches!(exprs.get(init_eid.0 as usize), Some(Expr::Uninit));
        if !is_uninit {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        let mut found: Option<(usize, ExprId)> = None;
        while j < stmts.len() {
            if let Stmt::Expr(eid) = &stmts[j]
                && let Some(Expr::Assign { target, value }) = exprs.get(eid.0 as usize)
                && let Some(Expr::Ident(n)) = exprs.get(target.0 as usize)
                && n == &name
            {
                found = Some((j, *value));
                break;
            }
            // Don't reach into non-flat control-flow: an assignment in
            // a sibling block / if-branch doesn't lift to the outer
            // scope. Only adjacent flat stmts in the SAME Vec<Stmt>
            // count.
            j += 1;
        }
        if let Some((stmt_idx, value)) = found {
            // Splice value into the let's init, drop the assignment.
            if let Stmt::LetDecl { init, .. } = &mut stmts[i] {
                *init = value;
            }
            stmts.remove(stmt_idx);
        }
        i += 1;
    }
}
