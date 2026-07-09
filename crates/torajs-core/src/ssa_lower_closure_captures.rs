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

/// RFC 20260710 — every name that is the target of an `Assign` /
/// `PostIncr` anywhere in the program, including lifted closure
/// bodies (all top-level FnDecls, so a closure writing its captured
/// binding is covered by the top-level walk). By-name approximation,
/// same precision as the escape collector: over-collection promotes
/// a never-written same-named capture to a box — one extra
/// indirection, semantics unchanged.
pub(crate) fn collect_assigned_names(ast: &Ast) -> HashSet<String> {
    let mut out = HashSet::new();
    for s in &ast.stmts {
        collect_assigned_in_stmt(ast, s, &mut out);
    }
    out
}

fn collect_assigned_in_stmt(ast: &Ast, s: &Stmt, out: &mut HashSet<String>) {
    match s {
        Stmt::Expr(eid) | Stmt::Throw(eid) | Stmt::Yield(eid) => {
            collect_assigned_in_expr(ast, *eid, out)
        }
        Stmt::YieldInto { value, .. } => collect_assigned_in_expr(ast, *value, out),
        Stmt::Return(Some(eid)) => collect_assigned_in_expr(ast, *eid, out),
        Stmt::LetDecl { init, .. } => collect_assigned_in_expr(ast, *init, out),
        Stmt::FnDecl { body, .. } => {
            for s in body {
                collect_assigned_in_stmt(ast, s, out);
            }
        }
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_assigned_in_expr(ast, *cond, out);
            collect_assigned_in_stmt(ast, then_branch, out);
            if let Some(eb) = else_branch {
                collect_assigned_in_stmt(ast, eb, out);
            }
        }
        Stmt::While { cond, body } | Stmt::DoWhile { body, cond } => {
            collect_assigned_in_expr(ast, *cond, out);
            collect_assigned_in_stmt(ast, body, out);
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            if let Some(i) = init {
                collect_assigned_in_stmt(ast, i, out);
            }
            if let Some(c) = cond {
                collect_assigned_in_expr(ast, *c, out);
            }
            if let Some(st) = step {
                collect_assigned_in_expr(ast, *st, out);
            }
            collect_assigned_in_stmt(ast, body, out);
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            for s in stmts {
                collect_assigned_in_stmt(ast, s, out);
            }
        }
        Stmt::Switch {
            scrutinee,
            cases,
            default,
        } => {
            collect_assigned_in_expr(ast, *scrutinee, out);
            for c in cases {
                for s in &c.body {
                    collect_assigned_in_stmt(ast, s, out);
                }
            }
            if let Some(d) = default {
                for s in d {
                    collect_assigned_in_stmt(ast, s, out);
                }
            }
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            for s in body.iter().chain(catch_body.iter()) {
                collect_assigned_in_stmt(ast, s, out);
            }
            if let Some(fb) = finally_body {
                for s in fb {
                    collect_assigned_in_stmt(ast, s, out);
                }
            }
        }
        _ => {}
    }
}

fn collect_assigned_in_expr(ast: &Ast, eid: ExprId, out: &mut HashSet<String>) {
    match ast.get_expr(eid) {
        Expr::Assign { target, value } => {
            if let Expr::Ident(n) = ast.get_expr(*target) {
                out.insert(n.clone());
            } else {
                collect_assigned_in_expr(ast, *target, out);
            }
            collect_assigned_in_expr(ast, *value, out);
        }
        Expr::PostIncr { target, .. } => {
            if let Expr::Ident(n) = ast.get_expr(*target) {
                out.insert(n.clone());
            }
        }
        Expr::ArrowFn { body, .. } => {
            for s in body {
                collect_assigned_in_stmt(ast, s, out);
            }
        }
        Expr::BinOp { left, right, .. } => {
            collect_assigned_in_expr(ast, *left, out);
            collect_assigned_in_expr(ast, *right, out);
        }
        Expr::Unary { expr, .. }
        | Expr::TypeOf { expr }
        | Expr::Spread { expr }
        | Expr::InstanceOf { expr, .. }
        | Expr::As { expr, .. } => collect_assigned_in_expr(ast, *expr, out),
        Expr::Member { obj, .. } | Expr::OptChain { obj, .. } => {
            collect_assigned_in_expr(ast, *obj, out)
        }
        Expr::Index { obj, index } | Expr::OptIndex { obj, index } => {
            collect_assigned_in_expr(ast, *obj, out);
            collect_assigned_in_expr(ast, *index, out);
        }
        Expr::Call { callee, args } | Expr::OptCall { callee, args } => {
            collect_assigned_in_expr(ast, *callee, out);
            for a in args {
                collect_assigned_in_expr(ast, *a, out);
            }
        }
        Expr::Array(els) => {
            for e in els {
                collect_assigned_in_expr(ast, *e, out);
            }
        }
        Expr::ObjectLit { fields } => {
            for (_, e) in fields {
                collect_assigned_in_expr(ast, *e, out);
            }
        }
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_assigned_in_expr(ast, *cond, out);
            collect_assigned_in_expr(ast, *then_branch, out);
            collect_assigned_in_expr(ast, *else_branch, out);
        }
        Expr::Nullish { lhs, rhs }
        | Expr::Sequence {
            left: lhs,
            right: rhs,
        } => {
            collect_assigned_in_expr(ast, *lhs, out);
            collect_assigned_in_expr(ast, *rhs, out);
        }
        Expr::New { args, .. } | Expr::Super { args } => {
            for e in args {
                collect_assigned_in_expr(ast, *e, out);
            }
        }
        _ => {}
    }
}

/// Chunk 725 — true when the expression tree contains a (lifted)
/// closure literal anywhere. The for-loop per-iteration rewrite
/// bails on init expressions that construct closures (an init
/// closure captures the INITIAL binding per ES §14.7.4, not a
/// per-iteration copy).
pub(crate) fn expr_contains_closure(ast: &Ast, eid: ExprId) -> bool {
    match ast.get_expr(eid) {
        Expr::Closure { .. } => true,
        Expr::As { expr, .. }
        | Expr::Unary { expr, .. }
        | Expr::TypeOf { expr }
        | Expr::Spread { expr }
        | Expr::InstanceOf { expr, .. } => expr_contains_closure(ast, *expr),
        Expr::BinOp { left, right, .. } => {
            expr_contains_closure(ast, *left) || expr_contains_closure(ast, *right)
        }
        Expr::Member { obj, .. } | Expr::OptChain { obj, .. } => expr_contains_closure(ast, *obj),
        Expr::OptIndex { obj, index } | Expr::Index { obj, index } => {
            expr_contains_closure(ast, *obj) || expr_contains_closure(ast, *index)
        }
        Expr::Call { callee, args } | Expr::OptCall { callee, args } => {
            expr_contains_closure(ast, *callee)
                || args.iter().any(|a| expr_contains_closure(ast, *a))
        }
        Expr::Assign { target, value } => {
            expr_contains_closure(ast, *target) || expr_contains_closure(ast, *value)
        }
        Expr::Array(els) => els.iter().any(|e| expr_contains_closure(ast, *e)),
        Expr::ObjectLit { fields } => fields.iter().any(|(_, e)| expr_contains_closure(ast, *e)),
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            expr_contains_closure(ast, *cond)
                || expr_contains_closure(ast, *then_branch)
                || expr_contains_closure(ast, *else_branch)
        }
        Expr::Nullish { lhs, rhs } => {
            expr_contains_closure(ast, *lhs) || expr_contains_closure(ast, *rhs)
        }
        Expr::New { args, .. } | Expr::Super { args } => {
            args.iter().any(|e| expr_contains_closure(ast, *e))
        }
        Expr::PostIncr { target, .. } => expr_contains_closure(ast, *target),
        _ => false,
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
