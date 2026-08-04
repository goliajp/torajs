//! Structural walkers for [`super::desugar_async_sm`].
//!
//! Both matches are deliberately EXHAUSTIVE — no `_ =>` arm. A new
//! `Expr` or `Stmt` variant then fails to compile here instead of
//! silently reading as "holds no await", which would let blade 1 move
//! a function whose suspend point it cannot place. A wrong answer in
//! that direction drops the suspend; a compile error is the cheap one.

use super::{Ast, Expr, ExprId, Stmt};

/// Call `f` on each direct child expression of `eid`. Nested function
/// bodies (`ArrowFn`) are NOT descended into: an `await` there belongs
/// to that function, not to the one being moved.
pub(super) fn visit_expr_children(ast: &Ast, eid: ExprId, f: &mut impl FnMut(ExprId)) {
    match ast.get_expr(eid) {
        Expr::Ident(_)
        | Expr::String(_)
        | Expr::Number(_)
        | Expr::BigInt { .. }
        | Expr::Bool(_)
        | Expr::Uninit
        | Expr::Regex { .. }
        | Expr::Null
        | Expr::Closure { .. }
        | Expr::This
        | Expr::NewTarget
        | Expr::Elision => {}
        Expr::BinOp { left, right, .. } | Expr::Sequence { left, right } => {
            f(*left);
            f(*right);
        }
        Expr::Nullish { lhs, rhs } => {
            f(*lhs);
            f(*rhs);
        }
        Expr::Unary { expr, .. }
        | Expr::As { expr, .. }
        | Expr::TypeOf { expr }
        | Expr::Delete { expr }
        | Expr::InstanceOf { expr, .. }
        | Expr::Spread { expr } => f(*expr),
        Expr::Member { obj, .. } | Expr::OptChain { obj, .. } => f(*obj),
        Expr::Index { obj, index } | Expr::OptIndex { obj, index } => {
            f(*obj);
            f(*index);
        }
        Expr::Call { callee, args }
        | Expr::OptCall { callee, args }
        | Expr::NewDynamic { callee, args } => {
            f(*callee);
            for a in args {
                f(*a);
            }
        }
        Expr::New { args, .. } | Expr::Super { args } | Expr::Array(args) => {
            for a in args {
                f(*a);
            }
        }
        Expr::Assign { target, value } => {
            f(*target);
            f(*value);
        }
        Expr::ObjectLit { fields } => {
            for (_, v) in fields {
                f(*v);
            }
        }
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            f(*cond);
            f(*then_branch);
            f(*else_branch);
        }
        Expr::PostIncr { target, .. } => f(*target),
        // A nested function's awaits are its own.
        Expr::ArrowFn { .. } => {}
    }
}

/// Call `f` on every expression a statement evaluates in its OWN
/// frame, descending through nested statements. Declaration bodies
/// (`FnDecl`, `ClassDecl`) are skipped for the same reason `ArrowFn`
/// is above.
pub(super) fn visit_stmt_exprs(s: &Stmt, f: &mut impl FnMut(ExprId)) {
    match s {
        Stmt::Expr(e) | Stmt::Throw(e) | Stmt::Yield(e) => f(*e),
        Stmt::Return(v) => {
            if let Some(e) = v {
                f(*e);
            }
        }
        Stmt::LetDecl { init, .. } => f(*init),
        Stmt::YieldInto { value, .. } => f(*value),
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            f(*cond);
            visit_stmt_exprs(then_branch, f);
            if let Some(b) = else_branch {
                visit_stmt_exprs(b, f);
            }
        }
        Stmt::While { cond, body } | Stmt::DoWhile { cond, body } => {
            f(*cond);
            visit_stmt_exprs(body, f);
        }
        Stmt::Switch {
            scrutinee,
            cases,
            default,
        } => {
            f(*scrutinee);
            for c in cases {
                f(c.value);
                for st in &c.body {
                    visit_stmt_exprs(st, f);
                }
            }
            if let Some(d) = default {
                for st in d {
                    visit_stmt_exprs(st, f);
                }
            }
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            if let Some(i) = init {
                visit_stmt_exprs(i, f);
            }
            if let Some(c) = cond {
                f(*c);
            }
            if let Some(st) = step {
                f(*st);
            }
            visit_stmt_exprs(body, f);
        }
        Stmt::ForOfSplitIter {
            parent, sep, body, ..
        } => {
            f(*parent);
            f(*sep);
            visit_stmt_exprs(body, f);
        }
        Stmt::ForOf {
            elem_expr,
            body,
            forin_obj,
            ..
        } => {
            f(*elem_expr);
            if let Some(o) = forin_obj {
                f(*o);
            }
            visit_stmt_exprs(body, f);
        }
        Stmt::Labeled { body, .. } => visit_stmt_exprs(body, f),
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            for st in stmts {
                visit_stmt_exprs(st, f);
            }
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            for st in body {
                visit_stmt_exprs(st, f);
            }
            for st in catch_body {
                visit_stmt_exprs(st, f);
            }
            if let Some(fin) = finally_body {
                for st in fin {
                    visit_stmt_exprs(st, f);
                }
            }
        }
        Stmt::ExportDecl {
            inner,
            default_expr,
            ..
        } => {
            if let Some(i) = inner {
                visit_stmt_exprs(i, f);
            }
            if let Some(e) = default_expr {
                f(*e);
            }
        }
        Stmt::Break(_)
        | Stmt::Continue(_)
        | Stmt::TypeDecl { .. }
        | Stmt::ImportDecl { .. }
        | Stmt::FnDecl { .. }
        | Stmt::ClassDecl { .. } => {}
    }
}
