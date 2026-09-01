//! `__tvdefault__<T>` marker rewrite — the third stage of the M3
//! generics monomorphization family (see
//! [`crate::ssa_lower_generics_monomorph`] module doc), split to its
//! own sibling at the 500-line file limit (chunk 705). Replaces the
//! marker Idents planted by class-factory default-init with the
//! concrete default expression for the substituted type, in place on
//! the AST arena.

use crate::ast::{Ast, Expr, ExprId, Stmt};

/// Walk a Stmt's expression graph and rewrite any `__tvdefault__<T>`
/// marker Ident into the proper concrete default expression for the
/// substituted type T. Operates IN PLACE on the AST arena (so the
/// caller's deep-cloned body sees the rewrite).
pub(crate) fn rewrite_tvdefault_in_stmt(ast: &mut Ast, s: &Stmt, subst: &[(String, String)]) {
    match s {
        Stmt::Expr(eid) | Stmt::Throw(eid) => rewrite_tvdefault_in_expr(ast, *eid, subst),
        Stmt::Return(maybe) => {
            if let Some(eid) = maybe {
                rewrite_tvdefault_in_expr(ast, *eid, subst);
            }
        }
        Stmt::LetDecl { init, .. } => rewrite_tvdefault_in_expr(ast, *init, subst),
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            rewrite_tvdefault_in_expr(ast, *cond, subst);
            rewrite_tvdefault_in_stmt(ast, then_branch, subst);
            if let Some(eb) = else_branch {
                rewrite_tvdefault_in_stmt(ast, eb, subst);
            }
        }
        Stmt::Labeled { body, .. } => {
            rewrite_tvdefault_in_stmt(ast, body, subst);
        }
        Stmt::While { cond, body } => {
            rewrite_tvdefault_in_expr(ast, *cond, subst);
            rewrite_tvdefault_in_stmt(ast, body, subst);
        }
        Stmt::DoWhile { body, cond } => {
            rewrite_tvdefault_in_stmt(ast, body, subst);
            rewrite_tvdefault_in_expr(ast, *cond, subst);
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            if let Some(i) = init {
                rewrite_tvdefault_in_stmt(ast, i, subst);
            }
            if let Some(c) = cond {
                rewrite_tvdefault_in_expr(ast, *c, subst);
            }
            if let Some(s2) = step {
                rewrite_tvdefault_in_expr(ast, *s2, subst);
            }
            rewrite_tvdefault_in_stmt(ast, body, subst);
        }
        Stmt::Switch {
            scrutinee,
            cases,
            default,
        } => {
            rewrite_tvdefault_in_expr(ast, *scrutinee, subst);
            for c in cases {
                rewrite_tvdefault_in_expr(ast, c.value, subst);
                for s in &c.body {
                    rewrite_tvdefault_in_stmt(ast, s, subst);
                }
            }
            if let Some(db) = default {
                for s in db {
                    rewrite_tvdefault_in_stmt(ast, s, subst);
                }
            }
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            for s in stmts {
                rewrite_tvdefault_in_stmt(ast, s, subst);
            }
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            for s in body {
                rewrite_tvdefault_in_stmt(ast, s, subst);
            }
            for s in catch_body {
                rewrite_tvdefault_in_stmt(ast, s, subst);
            }
            if let Some(fb) = finally_body {
                for s in fb {
                    rewrite_tvdefault_in_stmt(ast, s, subst);
                }
            }
        }
        _ => {}
    }
}

fn rewrite_tvdefault_in_expr(ast: &mut Ast, eid: ExprId, subst: &[(String, String)]) {
    // First detect the marker; rewrite in place if found.
    if let Expr::Ident(name) = ast.get_expr(eid) {
        if let Some(tv) = name.strip_prefix("__tvdefault__") {
            // Find the substituted concrete type for this TypeVar.
            for (tp_name, ann) in subst {
                if tp_name == tv {
                    let new_expr = match ann.as_str() {
                        "number" | "i64" => Expr::Number(0.0),
                        "f64" => Expr::Number(0.5), // forces fract() != 0 → ConstF64
                        "boolean" => Expr::Bool(false),
                        "string" => Expr::String(String::new().into()),
                        _ => Expr::Number(0.0),
                    };
                    ast.exprs[eid.0 as usize] = new_expr;
                    return;
                }
            }
        }
    }
    // Recurse into sub-expressions.
    let kind = ast.get_expr(eid).clone();
    match kind {
        Expr::BinOp { left, right, .. } => {
            rewrite_tvdefault_in_expr(ast, left, subst);
            rewrite_tvdefault_in_expr(ast, right, subst);
        }
        Expr::Unary { expr, .. } | Expr::TypeOf { expr } | Expr::Spread { expr } => {
            rewrite_tvdefault_in_expr(ast, expr, subst);
        }
        Expr::InstanceOf { expr, rhs } => {
            rewrite_tvdefault_in_expr(ast, expr, subst);
            rewrite_tvdefault_in_expr(ast, rhs, subst);
        }
        Expr::Member { obj, .. } | Expr::OptChain { obj, .. } => {
            rewrite_tvdefault_in_expr(ast, obj, subst);
        }
        Expr::OptIndex { obj, index } => {
            rewrite_tvdefault_in_expr(ast, obj, subst);
            rewrite_tvdefault_in_expr(ast, index, subst);
        }
        Expr::OptCall { callee, args } => {
            rewrite_tvdefault_in_expr(ast, callee, subst);
            for a in args {
                rewrite_tvdefault_in_expr(ast, a, subst);
            }
        }
        Expr::Call { callee, args } => {
            rewrite_tvdefault_in_expr(ast, callee, subst);
            for a in args {
                rewrite_tvdefault_in_expr(ast, a, subst);
            }
        }
        Expr::Assign { target, value } => {
            rewrite_tvdefault_in_expr(ast, target, subst);
            rewrite_tvdefault_in_expr(ast, value, subst);
        }
        Expr::Index { obj, index } => {
            rewrite_tvdefault_in_expr(ast, obj, subst);
            rewrite_tvdefault_in_expr(ast, index, subst);
        }
        Expr::Array(els) => {
            for e in els {
                rewrite_tvdefault_in_expr(ast, e, subst);
            }
        }
        Expr::ObjectLit { fields } => {
            for (_, e) in fields {
                rewrite_tvdefault_in_expr(ast, e, subst);
            }
        }
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            rewrite_tvdefault_in_expr(ast, cond, subst);
            rewrite_tvdefault_in_expr(ast, then_branch, subst);
            rewrite_tvdefault_in_expr(ast, else_branch, subst);
        }
        Expr::Nullish { lhs, rhs } => {
            rewrite_tvdefault_in_expr(ast, lhs, subst);
            rewrite_tvdefault_in_expr(ast, rhs, subst);
        }
        Expr::New { args, .. } | Expr::Super { args } => {
            for a in args {
                rewrite_tvdefault_in_expr(ast, a, subst);
            }
        }
        Expr::PostIncr { target, .. } => {
            rewrite_tvdefault_in_expr(ast, target, subst);
        }
        _ => {}
    }
}
