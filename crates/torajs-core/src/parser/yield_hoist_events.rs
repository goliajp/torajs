//! Evaluation-order events for the yield hoist — the guard walk that
//! answers whether hoisting a `yield` in front of its statement
//! reorders a side effect past it, and the temp-read probes the
//! destructuring-assignment desugar asks. Split verbatim out of
//! `yield_expr_hoist.rs` (which keeps the hoist itself) at the
//! 500-line cap.

use super::*;

/// Evaluation-order events collected by the guard walk. A temp read
/// carries its `__yx_<n>` name so the destructuring-default lane can
/// recover the matching `YieldInto` from the hoist buffer.
#[derive(PartialEq)]
enum Ev {
    Temp(String),
    SideEffect,
}

/// The `__yx_` temp names an expression reads, in source evaluation
/// order — the recovery key the destructuring-ASSIGNMENT desugar uses
/// to move a default's `YieldInto` stmts out of the hoist buffer and
/// into the conditional-default branch (the cover grammar parses
/// `[a = yield] = src` as a plain Assign rhs, where the hoist cannot
/// see that the position is conditional).
pub(super) fn expr_yield_temps(ast: &Ast, eid: ExprId) -> Vec<String> {
    let mut evs = Vec::new();
    expr_events(ast, eid, &mut evs);
    evs.into_iter()
        .filter_map(|e| match e {
            Ev::Temp(n) => Some(n),
            Ev::SideEffect => None,
        })
        .collect()
}

pub(super) fn check_hoist_eval_order(p: &Parser, stmt: &Stmt) -> Result<(), String> {
    let mut evs: Vec<Ev> = Vec::new();
    stmt_events(&p.ast, stmt, &mut evs);
    let last_temp = evs.iter().rposition(|e| matches!(e, Ev::Temp(_)));
    let first_se = evs.iter().position(|e| *e == Ev::SideEffect);
    if let (Some(t), Some(s)) = (last_temp, first_se)
        && s < t
    {
        return Err(format!(
            "not yet supported: a side effect is evaluated before `yield` in this \
             statement — hoisting the yield would reorder them at {}",
            p.at()
        ));
    }
    Ok(())
}

/// Append this expression's evaluation-order events. Child order
/// follows source evaluation order; effectful nodes emit their
/// SideEffect AFTER their children (a call fires after callee + args
/// evaluate, an assignment writes after its value evaluates).
fn expr_events(ast: &Ast, eid: ExprId, evs: &mut Vec<Ev>) {
    match ast.get_expr(eid) {
        Expr::Ident(n) => {
            if n.starts_with("__yx_") {
                evs.push(Ev::Temp(n.clone()));
            }
        }
        Expr::String(_)
        | Expr::Number(_)
        | Expr::BigInt { .. }
        | Expr::Bool(_)
        | Expr::Uninit
        | Expr::Regex { .. }
        | Expr::Null
        | Expr::Elision
        | Expr::This
        | Expr::NewTarget
        | Expr::Closure { .. } => {}
        Expr::BinOp { left, right, .. }
        | Expr::Sequence { left, right }
        | Expr::Nullish {
            lhs: left,
            rhs: right,
        }
        | Expr::Index {
            obj: left,
            index: right,
        }
        | Expr::OptIndex {
            obj: left,
            index: right,
        } => {
            expr_events(ast, *left, evs);
            expr_events(ast, *right, evs);
        }
        Expr::Unary { expr, .. }
        | Expr::TypeOf { expr }
        | Expr::Spread { expr }
        | Expr::As { expr, .. }
        | Expr::Member { obj: expr, .. }
        | Expr::OptChain { obj: expr, .. } => expr_events(ast, *expr, evs),
        Expr::InstanceOf { expr, rhs } => {
            expr_events(ast, *expr, evs);
            expr_events(ast, *rhs, evs);
        }
        Expr::Delete { expr } => {
            expr_events(ast, *expr, evs);
            evs.push(Ev::SideEffect);
        }
        Expr::PostIncr { target, .. } => {
            expr_events(ast, *target, evs);
            evs.push(Ev::SideEffect);
        }
        Expr::Assign { target, value } => {
            expr_events(ast, *target, evs);
            expr_events(ast, *value, evs);
            evs.push(Ev::SideEffect);
        }
        Expr::Call { callee, args }
        | Expr::OptCall { callee, args }
        | Expr::NewDynamic { callee, args } => {
            expr_events(ast, *callee, evs);
            for a in args {
                expr_events(ast, *a, evs);
            }
            evs.push(Ev::SideEffect);
        }
        Expr::New { args, .. } | Expr::Super { args } => {
            for a in args {
                expr_events(ast, *a, evs);
            }
            evs.push(Ev::SideEffect);
        }
        Expr::Array(items) => {
            for a in items {
                expr_events(ast, *a, evs);
            }
        }
        Expr::ObjectLit { fields } => {
            for (_, v) in fields {
                expr_events(ast, *v, evs);
            }
        }
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            expr_events(ast, *cond, evs);
            expr_events(ast, *then_branch, evs);
            expr_events(ast, *else_branch, evs);
        }
        // A function body's effects run at CALL time, not at
        // definition time — no events from inside.
        Expr::ArrowFn { params, .. } => {
            for p in params {
                if let Some(d) = p.default {
                    expr_events(ast, d, evs);
                }
            }
        }
    }
}

fn stmt_events(ast: &Ast, s: &Stmt, evs: &mut Vec<Ev>) {
    match s {
        Stmt::Expr(e) | Stmt::Throw(e) | Stmt::Yield(e) => expr_events(ast, *e, evs),
        Stmt::LetDecl { init, .. } | Stmt::UsingDecl { init, .. } => expr_events(ast, *init, evs),
        Stmt::YieldInto { value, .. } => expr_events(ast, *value, evs),
        Stmt::Return(e) => {
            if let Some(e) = e {
                expr_events(ast, *e, evs);
            }
        }
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            expr_events(ast, *cond, evs);
            stmt_events(ast, then_branch, evs);
            if let Some(e) = else_branch {
                stmt_events(ast, e, evs);
            }
        }
        Stmt::While { cond, body } | Stmt::DoWhile { body, cond } => {
            expr_events(ast, *cond, evs);
            stmt_events(ast, body, evs);
        }
        Stmt::Switch {
            scrutinee,
            cases,
            default,
        } => {
            expr_events(ast, *scrutinee, evs);
            for c in cases {
                expr_events(ast, c.value, evs);
                for s in &c.body {
                    stmt_events(ast, s, evs);
                }
            }
            if let Some(b) = default {
                for s in b {
                    stmt_events(ast, s, evs);
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
                stmt_events(ast, i, evs);
            }
            if let Some(c) = cond {
                expr_events(ast, *c, evs);
            }
            if let Some(st) = step {
                expr_events(ast, *st, evs);
            }
            stmt_events(ast, body, evs);
        }
        Stmt::ForOfSplitIter {
            parent, sep, body, ..
        } => {
            expr_events(ast, *parent, evs);
            expr_events(ast, *sep, evs);
            stmt_events(ast, body, evs);
        }
        Stmt::ForOf {
            elem_expr,
            body,
            forin_obj,
            ..
        } => {
            expr_events(ast, *elem_expr, evs);
            if let Some(o) = forin_obj {
                expr_events(ast, *o, evs);
            }
            stmt_events(ast, body, evs);
        }
        Stmt::Labeled { body, .. } => stmt_events(ast, body, evs),
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            for s in body {
                stmt_events(ast, s, evs);
            }
            for s in catch_body {
                stmt_events(ast, s, evs);
            }
            if let Some(b) = finally_body {
                for s in b {
                    stmt_events(ast, s, evs);
                }
            }
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            for s in stmts {
                stmt_events(ast, s, evs);
            }
        }
        Stmt::ExportDecl { inner, .. } => {
            if let Some(s) = inner.as_deref() {
                stmt_events(ast, s, evs);
            }
        }
        // Function/class bodies run later; decls carry no immediate
        // expression evaluation the guard needs to order.
        Stmt::FnDecl { .. }
        | Stmt::ClassDecl { .. }
        | Stmt::TypeDecl { .. }
        | Stmt::ImportDecl { .. }
        | Stmt::Break(_)
        | Stmt::Continue(_) => {}
    }
}
