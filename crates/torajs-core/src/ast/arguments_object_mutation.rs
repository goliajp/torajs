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

/// Which question the shared walk below is answering.
#[derive(Clone, Copy, PartialEq, Eq)]
enum MutScan {
    /// The FoldTo-admission scan (module doc): ANY mutation /
    /// escape / delete routes the body off the snapshot.
    View,
    /// The S3 Mapped-admission scan: param and in-range literal
    /// element writes are the POINT (they alias); only the shapes
    /// that need the materialized array disqualify.
    MappedBlocker,
}

/// True if the body can observe the difference between the FoldTo
/// snapshot and the real unmapped arguments object — see the module
/// doc for the three trigger classes.
pub(super) fn body_mutates_args_view(ast: &Ast, body: &[Stmt], params: &[String]) -> bool {
    body.iter()
        .any(|s| stmt_walk(ast, s, params, MutScan::View))
}

/// RFC 20260810-sloppy-goal-arguments S3 — what disqualifies a
/// sloppy simple-param static-face body from the Mapped rewrite
/// (the literal-index param substitution, whose two-way aliasing IS
/// §10.4.4 CreateMappedArgumentsObject's semantics): an escape, a
/// non-length member touch, an element delete, or a dynamic /
/// out-of-range index — those need the materialized array, and a
/// mapped body must never mix the two views (a dynamic write would
/// be invisible to a substituted literal read).
pub(super) fn body_has_mapped_blockers(ast: &Ast, body: &[Stmt], params: &[String]) -> bool {
    body.iter()
        .any(|s| stmt_walk(ast, s, params, MutScan::MappedBlocker))
}

fn is_arguments_ident(ast: &Ast, eid: ExprId) -> bool {
    matches!(ast.get_expr(eid), Expr::Ident(n) if n == "arguments")
}

fn is_write_target_hit(ast: &Ast, target: ExprId, params: &[String], what: MutScan) -> bool {
    match ast.get_expr(target) {
        // `arguments[i] = v` / `arguments[i]++` — element write.
        // Mapped ALLOWS the in-range literal spelling (the
        // substitution turns it into the aliasing param write);
        // a dynamic or out-of-range index still disqualifies.
        Expr::Index { obj, index } => {
            is_arguments_ident(ast, *obj)
                && (what == MutScan::View || !is_mapped_literal_index(ast, *index, params))
        }
        // `p = v` / `p++` on a user param — the aliasing IS the
        // Mapped semantics.
        Expr::Ident(n) => what == MutScan::View && params.iter().any(|p| p == n),
        _ => false,
    }
}

/// An integer Number literal the substitution arm covers (mirrors
/// the rewrite's `< params.len()` bound — extras included, which is
/// observationally exact either way: nothing else writes an extra).
fn is_mapped_literal_index(ast: &Ast, index: ExprId, params: &[String]) -> bool {
    matches!(ast.get_expr(index), Expr::Number(n)
        if n.fract() == 0.0 && (*n as usize) < params.len())
}

fn stmt_walk(ast: &Ast, s: &Stmt, params: &[String], what: MutScan) -> bool {
    match s {
        Stmt::Expr(eid) | Stmt::Throw(eid) | Stmt::Yield(eid) => expr_walk(ast, *eid, params, what),
        Stmt::Return(opt) => opt.is_some_and(|e| expr_walk(ast, e, params, what)),
        Stmt::LetDecl { init, .. } => expr_walk(ast, *init, params, what),
        Stmt::YieldInto { value, .. } => expr_walk(ast, *value, params, what),
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            expr_walk(ast, *cond, params, what)
                || stmt_walk(ast, then_branch, params, what)
                || else_branch
                    .as_ref()
                    .is_some_and(|e| stmt_walk(ast, e, params, what))
        }
        Stmt::While { cond, body } | Stmt::DoWhile { cond, body } => {
            expr_walk(ast, *cond, params, what) || stmt_walk(ast, body, params, what)
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            init.as_ref()
                .is_some_and(|s| stmt_walk(ast, s, params, what))
                || cond.is_some_and(|c| expr_walk(ast, c, params, what))
                || step.is_some_and(|st| expr_walk(ast, st, params, what))
                || stmt_walk(ast, body, params, what)
        }
        Stmt::ForOfSplitIter {
            parent, sep, body, ..
        } => {
            expr_walk(ast, *parent, params, what)
                || expr_walk(ast, *sep, params, what)
                || stmt_walk(ast, body, params, what)
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
            expr_walk(ast, *elem_expr, params, what) || stmt_walk(ast, body, params, what)
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            stmts.iter().any(|s| stmt_walk(ast, s, params, what))
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            body.iter().any(|s| stmt_walk(ast, s, params, what))
                || catch_body.iter().any(|s| stmt_walk(ast, s, params, what))
                || finally_body
                    .as_ref()
                    .is_some_and(|fb| fb.iter().any(|s| stmt_walk(ast, s, params, what)))
        }
        // Nested FnDecl owns its own arguments scope (lifted to
        // top-level before this pass; defensive).
        _ => false,
    }
}

fn expr_walk(ast: &Ast, eid: ExprId, params: &[String], what: MutScan) -> bool {
    match ast.get_expr(eid) {
        Expr::Assign { target, value } => {
            is_write_target_hit(ast, *target, params, what)
                || expr_walk(ast, *target, params, what)
                || expr_walk(ast, *value, params, what)
        }
        Expr::PostIncr { target, .. } => {
            is_write_target_hit(ast, *target, params, what) || expr_walk(ast, *target, params, what)
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
                || expr_walk(ast, *expr, params, what)
        }
        // `arguments[i]` read — absorbed (obj not recursed): the
        // substitution answer is the snapshot value, correct while
        // no write / escape exists elsewhere in the body. Mapped
        // additionally requires the literal in-range spelling — a
        // dynamic read would ride the materialized array, blind to
        // the substituted writes.
        Expr::Index { obj, index } if is_arguments_ident(ast, *obj) => {
            (what == MutScan::MappedBlocker && !is_mapped_literal_index(ast, *index, params))
                || expr_walk(ast, *index, params, what)
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
            expr_walk(ast, *obj, params, what)
        }
        Expr::Index { obj, index } | Expr::OptIndex { obj, index } => {
            expr_walk(ast, *obj, params, what) || expr_walk(ast, *index, params, what)
        }
        Expr::BinOp { left, right, .. } => {
            expr_walk(ast, *left, params, what) || expr_walk(ast, *right, params, what)
        }
        Expr::Call { callee, args } | Expr::OptCall { callee, args } => {
            expr_walk(ast, *callee, params, what)
                || args.iter().any(|a| expr_walk(ast, *a, params, what))
        }
        Expr::Array(items) => items.iter().any(|e| expr_walk(ast, *e, params, what)),
        Expr::ObjectLit { fields } => fields.iter().any(|(_, e)| expr_walk(ast, *e, params, what)),
        Expr::Spread { expr } => expr_walk(ast, *expr, params, what),
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            expr_walk(ast, *cond, params, what)
                || expr_walk(ast, *then_branch, params, what)
                || expr_walk(ast, *else_branch, params, what)
        }
        Expr::Nullish { lhs, rhs } => {
            expr_walk(ast, *lhs, params, what) || expr_walk(ast, *rhs, params, what)
        }
        Expr::OptChain { obj, .. } => expr_walk(ast, *obj, params, what),
        _ => false,
    }
}
