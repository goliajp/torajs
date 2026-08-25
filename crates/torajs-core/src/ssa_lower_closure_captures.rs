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

mod assigned;

pub(crate) use assigned::collect_assigned_names_scoped;

/// What one body's closure-construction walk finds (RFC
/// 20260731-mono-closure-clone 刀 2): the names its closures capture
/// (the escape-capture set) and the lifted fn names it constructs —
/// the latter scopes the assigned-name collection to exactly the
/// bodies that can write those captures.
#[derive(Default)]
pub(crate) struct ClosureScan {
    pub(crate) captures: HashSet<String>,
    pub(crate) fn_names: Vec<String>,
}

pub(crate) fn collect_closure_captures_in_stmt(ast: &Ast, s: &Stmt, out: &mut ClosureScan) {
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
        Stmt::Labeled { body, .. } => {
            collect_closure_captures_in_stmt(ast, body, out);
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
        Stmt::ForOf {
            elem_expr,
            body,
            forin_obj,
            ..
        } => {
            collect_closure_captures_in_expr(ast, *elem_expr, out);
            if let Some(fo) = forin_obj {
                collect_closure_captures_in_expr(ast, *fo, out);
            }
            collect_closure_captures_in_stmt(ast, body, out);
        }
        Stmt::ForOfSplitIter {
            parent, sep, body, ..
        } => {
            collect_closure_captures_in_expr(ast, *parent, out);
            collect_closure_captures_in_expr(ast, *sep, out);
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
        | Expr::Spread { expr } => expr_contains_closure(ast, *expr),
        Expr::InstanceOf { expr, rhs } => {
            expr_contains_closure(ast, *expr) || expr_contains_closure(ast, *rhs)
        }
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

fn collect_closure_captures_in_expr(ast: &Ast, eid: ExprId, out: &mut ClosureScan) {
    match ast.get_expr(eid) {
        Expr::Closure { fn_name, captures } => {
            for c in captures {
                out.captures.insert(c.clone());
            }
            out.fn_names.push(fn_name.clone());
        }
        Expr::BinOp { left, right, .. } => {
            collect_closure_captures_in_expr(ast, *left, out);
            collect_closure_captures_in_expr(ast, *right, out);
        }
        Expr::Unary { expr, .. } | Expr::TypeOf { expr } | Expr::Spread { expr } => {
            collect_closure_captures_in_expr(ast, *expr, out);
        }
        Expr::InstanceOf { expr, rhs } => {
            collect_closure_captures_in_expr(ast, *expr, out);
            collect_closure_captures_in_expr(ast, *rhs, out);
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
        // Wrappers that carry a closure through unchanged. `As` was
        // missing here (rotation 497): `f(x) as T` hid the callback
        // from this census, so the `let` it captured never got its
        // capture box — the env then treated the raw alloca as a box
        // (`capture_box_inc` / `_drop` on stack memory; the drop
        // landed on the env-drop callee's saved lr once the gate-open
        // frame layout put the slot at main's sp base). The catch-all
        // is gone: a new variant must be placed explicitly.
        Expr::As { expr, .. } | Expr::Delete { expr } => {
            collect_closure_captures_in_expr(ast, *expr, out);
        }
        Expr::Sequence { left, right } => {
            collect_closure_captures_in_expr(ast, *left, out);
            collect_closure_captures_in_expr(ast, *right, out);
        }
        Expr::NewDynamic { callee, args } => {
            collect_closure_captures_in_expr(ast, *callee, out);
            for a in args {
                collect_closure_captures_in_expr(ast, *a, out);
            }
        }
        // Leaves — and an un-lifted `ArrowFn`, whose body constructs
        // its closures in its OWN frame (every arrow this body
        // constructs has been lifted to `Closure` by lowering time).
        Expr::Ident(_)
        | Expr::String(_)
        | Expr::Number(_)
        | Expr::Bool(_)
        | Expr::BigInt { .. }
        | Expr::Regex { .. }
        | Expr::Null
        | Expr::This
        | Expr::NewTarget
        | Expr::Elision
        | Expr::Uninit
        | Expr::ArrowFn { .. } => {}
    }
}
