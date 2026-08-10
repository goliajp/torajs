//! The FoldTo-admission mutation scan for
//! [`super::arguments_object::desugar_arguments_object`] (rotation
//! 272). A static-argv-face body may keep the zero-cost FoldTo
//! rewrite (literal-index param substitution + inline spread
//! expansion) ONLY when that is observationally equivalent to the
//! unmapped arguments object every module-code (strict) fn really
//! has. Equivalence holds exactly when nothing can tell the
//! snapshot from the live view:
//!
//! - no `arguments[...]` write (element write must not reach the
//!   param, and must show in later arguments reads),
//! - no write to any user param (a param write must NOT show in a
//!   later `arguments[i]` read),
//! - no escape of the arguments object itself (a bare `arguments`
//!   value use or a non-length member touch hands out an aliasable
//!   object; writes through the alias are statically invisible).
//!
//! Any hit routes the body to ArgcMode::Unmapped, where every index
//! rides the materialized `__torajs_arguments` array.

use super::{Ast, Expr, ExprId, Stmt};

/// True if the body can observe the difference between the FoldTo
/// snapshot and the real unmapped arguments object — see the module
/// doc for the three trigger classes.
pub(super) fn body_mutates_args_view(ast: &Ast, body: &[Stmt], params: &[String]) -> bool {
    body.iter().any(|s| stmt_walk(ast, s, params))
}

fn is_arguments_ident(ast: &Ast, eid: ExprId) -> bool {
    matches!(ast.get_expr(eid), Expr::Ident(n) if n == "arguments")
}

fn is_write_target_hit(ast: &Ast, target: ExprId, params: &[String]) -> bool {
    match ast.get_expr(target) {
        // `arguments[i] = v` / `arguments[i]++` — element write.
        Expr::Index { obj, .. } => is_arguments_ident(ast, *obj),
        // `p = v` / `p++` on a user param.
        Expr::Ident(n) => params.iter().any(|p| p == n),
        _ => false,
    }
}

fn stmt_walk(ast: &Ast, s: &Stmt, params: &[String]) -> bool {
    match s {
        Stmt::Expr(eid) | Stmt::Throw(eid) | Stmt::Yield(eid) => expr_walk(ast, *eid, params),
        Stmt::Return(opt) => opt.is_some_and(|e| expr_walk(ast, e, params)),
        Stmt::LetDecl { init, .. } => expr_walk(ast, *init, params),
        Stmt::YieldInto { value, .. } => expr_walk(ast, *value, params),
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            expr_walk(ast, *cond, params)
                || stmt_walk(ast, then_branch, params)
                || else_branch
                    .as_ref()
                    .is_some_and(|e| stmt_walk(ast, e, params))
        }
        Stmt::While { cond, body } | Stmt::DoWhile { cond, body } => {
            expr_walk(ast, *cond, params) || stmt_walk(ast, body, params)
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            init.as_ref().is_some_and(|s| stmt_walk(ast, s, params))
                || cond.is_some_and(|c| expr_walk(ast, c, params))
                || step.is_some_and(|st| expr_walk(ast, st, params))
                || stmt_walk(ast, body, params)
        }
        Stmt::ForOfSplitIter {
            parent, sep, body, ..
        } => {
            expr_walk(ast, *parent, params)
                || expr_walk(ast, *sep, params)
                || stmt_walk(ast, body, params)
        }
        Stmt::ForOf {
            var_name,
            elem_expr,
            body,
            ..
        } => {
            // The loop variable rebinds each iteration; a same-named
            // param is shadowed inside, but treating its writes as
            // param writes only loses a FoldTo admit — never wrong.
            let _ = var_name;
            expr_walk(ast, *elem_expr, params) || stmt_walk(ast, body, params)
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => stmts.iter().any(|s| stmt_walk(ast, s, params)),
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            body.iter().any(|s| stmt_walk(ast, s, params))
                || catch_body.iter().any(|s| stmt_walk(ast, s, params))
                || finally_body
                    .as_ref()
                    .is_some_and(|fb| fb.iter().any(|s| stmt_walk(ast, s, params)))
        }
        // Nested FnDecl owns its own arguments scope (lifted to
        // top-level before this pass; defensive).
        _ => false,
    }
}

fn expr_walk(ast: &Ast, eid: ExprId, params: &[String]) -> bool {
    match ast.get_expr(eid) {
        Expr::Assign { target, value } => {
            is_write_target_hit(ast, *target, params)
                || expr_walk(ast, *target, params)
                || expr_walk(ast, *value, params)
        }
        Expr::PostIncr { target, .. } => {
            is_write_target_hit(ast, *target, params) || expr_walk(ast, *target, params)
        }
        // `delete arguments[i]` — a MUTATION (§10.4.4.6 unmapped
        // elements are plain deletable data properties): the
        // snapshot substitution would turn the target into a bare
        // param name (10.5-7-b-4-s's "must be a property reference"
        // reject). Routes the body to the materialized array.
        Expr::Delete { expr } => {
            matches!(ast.get_expr(*expr), Expr::Index { obj, .. }
                if is_arguments_ident(ast, *obj))
                || matches!(ast.get_expr(*expr), Expr::Member { obj, name }
                    if name == "length" && is_arguments_ident(ast, *obj))
                || expr_walk(ast, *expr, params)
        }
        // `arguments[i]` read — absorbed (obj not recursed): the
        // substitution answer is the snapshot value, correct while
        // no write / escape exists elsewhere in the body.
        Expr::Index { obj, index } if is_arguments_ident(ast, *obj) => {
            expr_walk(ast, *index, params)
        }
        // `arguments.length` — absorbed (folds to the static argc).
        Expr::Member { obj, name } if name == "length" && is_arguments_ident(ast, *obj) => false,
        // Any OTHER member touch (`arguments.callee`, …) hands out /
        // observes the object itself — no snapshot equivalence.
        Expr::Member { obj, .. } if is_arguments_ident(ast, *obj) => true,
        // `...arguments` spread — absorbed: the inline expansion is
        // the snapshot, correct under the same no-write condition.
        Expr::Spread { expr } if is_arguments_ident(ast, *expr) => false,
        // A bare `arguments` value use anywhere else is an escape —
        // the alias can be written through invisibly.
        Expr::Ident(n) if n == "arguments" => true,
        Expr::Member { obj, .. } | Expr::TypeOf { expr: obj } | Expr::Unary { expr: obj, .. } => {
            expr_walk(ast, *obj, params)
        }
        Expr::Index { obj, index } | Expr::OptIndex { obj, index } => {
            expr_walk(ast, *obj, params) || expr_walk(ast, *index, params)
        }
        Expr::BinOp { left, right, .. } => {
            expr_walk(ast, *left, params) || expr_walk(ast, *right, params)
        }
        Expr::Call { callee, args } | Expr::OptCall { callee, args } => {
            expr_walk(ast, *callee, params) || args.iter().any(|a| expr_walk(ast, *a, params))
        }
        Expr::Array(items) => items.iter().any(|e| expr_walk(ast, *e, params)),
        Expr::ObjectLit { fields } => fields.iter().any(|(_, e)| expr_walk(ast, *e, params)),
        Expr::Spread { expr } => expr_walk(ast, *expr, params),
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            expr_walk(ast, *cond, params)
                || expr_walk(ast, *then_branch, params)
                || expr_walk(ast, *else_branch, params)
        }
        Expr::Nullish { lhs, rhs } => expr_walk(ast, *lhs, params) || expr_walk(ast, *rhs, params),
        Expr::OptChain { obj, .. } => expr_walk(ast, *obj, params),
        _ => false,
    }
}
