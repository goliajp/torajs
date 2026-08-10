//! The statement-form walker half of
//! [`super::arguments_object_rewrite`] — split to a sibling when the
//! S2 sloppy-callee threading (RFC 20260810-sloppy-goal-arguments)
//! pushed the rewriter past the 500-line file limit. Verbatim move;
//! the parent re-exports `rewrite_arguments_in_stmt` so the main
//! pass's call site is unchanged.

use super::arguments_object::ArgcMode;
use super::arguments_object_rewrite::{SloppyCallee, rewrite_arguments_in_expr};
use super::{Ast, Stmt};

pub(super) fn rewrite_arguments_in_stmt(
    ast: &mut Ast,
    s: &Stmt,
    params: &[String],
    argc_mode: ArgcMode,
    is_argv_fn: bool,
    sloppy_callee: SloppyCallee<'_>,
) -> Stmt {
    match s {
        Stmt::Expr(eid) => Stmt::Expr(rewrite_arguments_in_expr(
            ast,
            *eid,
            params,
            argc_mode,
            is_argv_fn,
            sloppy_callee,
        )),
        Stmt::Throw(eid) => Stmt::Throw(rewrite_arguments_in_expr(
            ast,
            *eid,
            params,
            argc_mode,
            is_argv_fn,
            sloppy_callee,
        )),
        Stmt::Return(Some(eid)) => Stmt::Return(Some(rewrite_arguments_in_expr(
            ast,
            *eid,
            params,
            argc_mode,
            is_argv_fn,
            sloppy_callee,
        ))),
        Stmt::Return(None) => Stmt::Return(None),
        Stmt::LetDecl {
            mutable,
            name,
            type_ann,
            init,
            is_var,
        } => Stmt::LetDecl {
            mutable: *mutable,
            name: name.clone(),
            type_ann: type_ann.clone(),
            init: rewrite_arguments_in_expr(
                ast,
                *init,
                params,
                argc_mode,
                is_argv_fn,
                sloppy_callee,
            ),
            // preserve `var` flag (hardcoded false dropped var-hoist
            // semantics on rewritten `var` decls; zero-warn surfaced it).
            is_var: *is_var,
        },
        Stmt::Block(stmts) => Stmt::Block(
            stmts
                .iter()
                .map(|s| {
                    rewrite_arguments_in_stmt(ast, s, params, argc_mode, is_argv_fn, sloppy_callee)
                })
                .collect(),
        ),
        Stmt::Multi(stmts) => Stmt::Multi(
            stmts
                .iter()
                .map(|s| {
                    rewrite_arguments_in_stmt(ast, s, params, argc_mode, is_argv_fn, sloppy_callee)
                })
                .collect(),
        ),
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => Stmt::If {
            cond: rewrite_arguments_in_expr(
                ast,
                *cond,
                params,
                argc_mode,
                is_argv_fn,
                sloppy_callee,
            ),
            then_branch: Box::new(rewrite_arguments_in_stmt(
                ast,
                then_branch,
                params,
                argc_mode,
                is_argv_fn,
                sloppy_callee,
            )),
            else_branch: else_branch.as_ref().map(|eb| {
                Box::new(rewrite_arguments_in_stmt(
                    ast,
                    eb,
                    params,
                    argc_mode,
                    is_argv_fn,
                    sloppy_callee,
                ))
            }),
        },
        Stmt::Labeled { label, body } => Stmt::Labeled {
            label: label.clone(),
            body: Box::new(rewrite_arguments_in_stmt(
                ast,
                body,
                params,
                argc_mode,
                is_argv_fn,
                sloppy_callee,
            )),
        },
        Stmt::While { cond, body } => Stmt::While {
            cond: rewrite_arguments_in_expr(
                ast,
                *cond,
                params,
                argc_mode,
                is_argv_fn,
                sloppy_callee,
            ),
            body: Box::new(rewrite_arguments_in_stmt(
                ast,
                body,
                params,
                argc_mode,
                is_argv_fn,
                sloppy_callee,
            )),
        },
        Stmt::DoWhile { cond, body } => Stmt::DoWhile {
            cond: rewrite_arguments_in_expr(
                ast,
                *cond,
                params,
                argc_mode,
                is_argv_fn,
                sloppy_callee,
            ),
            body: Box::new(rewrite_arguments_in_stmt(
                ast,
                body,
                params,
                argc_mode,
                is_argv_fn,
                sloppy_callee,
            )),
        },
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => Stmt::For {
            init: init.as_ref().map(|i| {
                Box::new(rewrite_arguments_in_stmt(
                    ast,
                    i,
                    params,
                    argc_mode,
                    is_argv_fn,
                    sloppy_callee,
                ))
            }),
            cond: cond.map(|c| {
                rewrite_arguments_in_expr(ast, c, params, argc_mode, is_argv_fn, sloppy_callee)
            }),
            step: step.map(|u| {
                rewrite_arguments_in_expr(ast, u, params, argc_mode, is_argv_fn, sloppy_callee)
            }),
            body: Box::new(rewrite_arguments_in_stmt(
                ast,
                body,
                params,
                argc_mode,
                is_argv_fn,
                sloppy_callee,
            )),
        },
        Stmt::Try {
            body,
            had_catch,
            catch_param,
            catch_type,
            catch_body,
            finally_body,
        } => Stmt::Try {
            body: body
                .iter()
                .map(|s| {
                    rewrite_arguments_in_stmt(ast, s, params, argc_mode, is_argv_fn, sloppy_callee)
                })
                .collect(),
            had_catch: *had_catch,
            catch_param: catch_param.clone(),
            catch_type: catch_type.clone(),
            catch_body: catch_body
                .iter()
                .map(|s| {
                    rewrite_arguments_in_stmt(ast, s, params, argc_mode, is_argv_fn, sloppy_callee)
                })
                .collect(),
            finally_body: finally_body.as_ref().map(|fb| {
                fb.iter()
                    .map(|s| {
                        rewrite_arguments_in_stmt(
                            ast,
                            s,
                            params,
                            argc_mode,
                            is_argv_fn,
                            sloppy_callee,
                        )
                    })
                    .collect()
            }),
        },
        // RFC 20260801 — for-of bodies were previously cloned opaque
        // (the `other` arm below), leaving `arguments` touches inside
        // the loop unrewritten → checker unknown-identifier. The
        // source itself is a parser-hoisted LetDecl (its init rides
        // the LetDecl arm above); only elem_expr + body need walking.
        Stmt::ForOf {
            var_name,
            var_type_ann,
            src_ident,
            i_ident,
            elem_expr,
            body,
            forin_obj,
            is_await,
        } => Stmt::ForOf {
            var_name: var_name.clone(),
            var_type_ann: var_type_ann.clone(),
            src_ident: src_ident.clone(),
            i_ident: i_ident.clone(),
            elem_expr: rewrite_arguments_in_expr(
                ast,
                *elem_expr,
                params,
                argc_mode,
                is_argv_fn,
                sloppy_callee,
            ),
            body: Box::new(rewrite_arguments_in_stmt(
                ast,
                body,
                params,
                argc_mode,
                is_argv_fn,
                sloppy_callee,
            )),
            forin_obj: *forin_obj,
            is_await: *is_await,
        },
        Stmt::ForOfSplitIter {
            var_name,
            parent,
            sep,
            body,
        } => Stmt::ForOfSplitIter {
            var_name: var_name.clone(),
            parent: rewrite_arguments_in_expr(
                ast,
                *parent,
                params,
                argc_mode,
                is_argv_fn,
                sloppy_callee,
            ),
            sep: rewrite_arguments_in_expr(ast, *sep, params, argc_mode, is_argv_fn, sloppy_callee),
            body: Box::new(rewrite_arguments_in_stmt(
                ast,
                body,
                params,
                argc_mode,
                is_argv_fn,
                sloppy_callee,
            )),
        },
        // Nested FnDecl owns its own arguments scope — leave it for
        // the outer pass to handle independently when it iterates
        // ast.stmts (lift_arrow_fns has already hoisted closures to
        // top-level FnDecls, so nested-FnDecl-in-body is rare in
        // practice).
        other => other.clone(),
    }
}
