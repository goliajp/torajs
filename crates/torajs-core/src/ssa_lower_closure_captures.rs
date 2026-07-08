//! AST escape-capture visitor — collects every name referenced
//! inside an `Expr::Closure { captures }` list.
//!
//! Used by `lower_fn`'s escape-capture pre-pass so the let-decl
//! path can heap-allocate slots for any binding that an escaping
//! closure will hold pointers to. Extracted out of `ssa_lower.rs`
//! to keep that file on the known-debt "only-shrinks" trajectory;
//! mirrors the placement chosen for the 11-A1 deque-escape visitor.

use std::collections::HashSet;

use crate::ast::{Ast, Expr, ExprId, Stmt};

pub(crate) fn collect_closure_captures_in_stmt(ast: &Ast, s: &Stmt, out: &mut HashSet<String>) {
    match s {
        Stmt::Expr(eid) | Stmt::Throw(eid) | Stmt::Yield(eid) => {
            collect_closure_captures_in_expr(ast, *eid, out)
        }
        Stmt::YieldInto { value, .. } => collect_closure_captures_in_expr(ast, *value, out),
        Stmt::Return(Some(eid)) => collect_closure_captures_in_expr(ast, *eid, out),
        Stmt::Return(None) => {}
        Stmt::LetDecl { init, .. } => collect_closure_captures_in_expr(ast, *init, out),
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_closure_captures_in_expr(ast, *cond, out);
            collect_closure_captures_in_stmt(ast, then_branch, out);
            if let Some(eb) = else_branch {
                collect_closure_captures_in_stmt(ast, eb, out);
            }
        }
        Stmt::While { cond, body } => {
            collect_closure_captures_in_expr(ast, *cond, out);
            collect_closure_captures_in_stmt(ast, body, out);
        }
        Stmt::DoWhile { body, cond } => {
            collect_closure_captures_in_stmt(ast, body, out);
            collect_closure_captures_in_expr(ast, *cond, out);
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            if let Some(i) = init {
                collect_closure_captures_in_stmt(ast, i, out);
            }
            if let Some(c) = cond {
                collect_closure_captures_in_expr(ast, *c, out);
            }
            if let Some(st) = step {
                collect_closure_captures_in_expr(ast, *st, out);
            }
            collect_closure_captures_in_stmt(ast, body, out);
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            for s in stmts {
                collect_closure_captures_in_stmt(ast, s, out);
            }
        }
        Stmt::Switch {
            scrutinee,
            cases,
            default,
        } => {
            collect_closure_captures_in_expr(ast, *scrutinee, out);
            for c in cases {
                collect_closure_captures_in_expr(ast, c.value, out);
                for s in &c.body {
                    collect_closure_captures_in_stmt(ast, s, out);
                }
            }
            if let Some(d) = default {
                for s in d {
                    collect_closure_captures_in_stmt(ast, s, out);
                }
            }
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            for s in body {
                collect_closure_captures_in_stmt(ast, s, out);
            }
            for s in catch_body {
                collect_closure_captures_in_stmt(ast, s, out);
            }
            if let Some(fb) = finally_body {
                for s in fb {
                    collect_closure_captures_in_stmt(ast, s, out);
                }
            }
        }
        _ => {}
    }
}

fn collect_closure_captures_in_expr(ast: &Ast, eid: ExprId, out: &mut HashSet<String>) {
    match ast.get_expr(eid) {
        Expr::Closure { captures, .. } => {
            for c in captures {
                out.insert(c.clone());
            }
        }
        Expr::BinOp { left, right, .. } => {
            collect_closure_captures_in_expr(ast, *left, out);
            collect_closure_captures_in_expr(ast, *right, out);
        }
        Expr::Unary { expr, .. }
        | Expr::TypeOf { expr }
        | Expr::Spread { expr }
        | Expr::InstanceOf { expr, .. } => {
            collect_closure_captures_in_expr(ast, *expr, out);
        }
        Expr::Member { obj, .. } | Expr::OptChain { obj, .. } => {
            collect_closure_captures_in_expr(ast, *obj, out);
        }
        Expr::OptIndex { obj, index } => {
            collect_closure_captures_in_expr(ast, *obj, out);
            collect_closure_captures_in_expr(ast, *index, out);
        }
        Expr::OptCall { callee, args } => {
            collect_closure_captures_in_expr(ast, *callee, out);
            for a in args {
                collect_closure_captures_in_expr(ast, *a, out);
            }
        }
        Expr::Call { callee, args } => {
            collect_closure_captures_in_expr(ast, *callee, out);
            for a in args {
                collect_closure_captures_in_expr(ast, *a, out);
            }
        }
        Expr::Assign { target, value } => {
            collect_closure_captures_in_expr(ast, *target, out);
            collect_closure_captures_in_expr(ast, *value, out);
        }
        Expr::Index { obj, index } => {
            collect_closure_captures_in_expr(ast, *obj, out);
            collect_closure_captures_in_expr(ast, *index, out);
        }
        Expr::Array(els) => {
            for e in els {
                collect_closure_captures_in_expr(ast, *e, out);
            }
        }
        Expr::ObjectLit { fields } => {
            for (_, e) in fields {
                collect_closure_captures_in_expr(ast, *e, out);
            }
        }
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_closure_captures_in_expr(ast, *cond, out);
            collect_closure_captures_in_expr(ast, *then_branch, out);
            collect_closure_captures_in_expr(ast, *else_branch, out);
        }
        Expr::Nullish { lhs, rhs } => {
            collect_closure_captures_in_expr(ast, *lhs, out);
            collect_closure_captures_in_expr(ast, *rhs, out);
        }
        Expr::New { args, .. } | Expr::Super { args } => {
            for e in args {
                collect_closure_captures_in_expr(ast, *e, out);
            }
        }
        Expr::PostIncr { target, .. } => {
            collect_closure_captures_in_expr(ast, *target, out);
        }
        _ => {}
    }
}
