//! Fill-site collection walk (chunk 784 extraction — the
//! direct-call-argument expression walk pushed
//! `fill_optional_fields.rs` past the 500-line file limit).

use std::collections::HashMap;

use crate::ast::{Ast, Expr, ExprId, PropKey, Stmt};

use super::{AliasEnv, resolve_struct_ann};

/// Chunk 788 — push a fill job for `lit_eid` against `declared`,
/// first recursing into struct-typed fields whose value is itself an
/// ObjectLit: `const h: H = { o: {} }` fills the INNER literal
/// against O's declared list too (the chunk-784 walk only jobbed the
/// outer literal, so the short inner literal stayed a loud checker
/// reject). An optional struct field's `__nullable(...)` wrapper
/// strips before resolving, mirroring the sentinel gate.
fn push_objlit_jobs(
    ast: &Ast,
    lit_eid: ExprId,
    declared: Vec<(PropKey, String)>,
    env: &AliasEnv,
    jobs: &mut Vec<(ExprId, Vec<(PropKey, String)>)>,
) {
    if let Expr::ObjectLit { fields } = ast.get_expr(lit_eid) {
        for (fname, feid) in fields {
            if matches!(ast.get_expr(*feid), Expr::ObjectLit { .. })
                && let Some((_, fann)) = declared.iter().find(|(n, _)| n == fname)
            {
                let inner = fann
                    .strip_prefix("__nullable(")
                    .and_then(|r| r.strip_suffix(')'))
                    .unwrap_or(fann);
                if let Some(fdecl) = resolve_struct_ann(inner.trim(), env) {
                    push_objlit_jobs(ast, *feid, fdecl, env, jobs);
                }
            }
        }
    }
    jobs.push((lit_eid, declared));
}

/// Walk statement trees collecting `(objlit_eid, declared_fields)`
/// pairs for annotated let-decls whose init is a plain ObjectLit,
/// plus direct-call argument sites via the expression walk
/// (chunk 784).
pub(super) fn collect_jobs(
    ast: &Ast,
    stmts: &[Stmt],
    env: &AliasEnv,
    fn_params: &HashMap<String, Vec<Option<String>>>,
    jobs: &mut Vec<(ExprId, Vec<(PropKey, String)>)>,
) {
    for s in stmts {
        match s {
            Stmt::LetDecl { type_ann, init, .. } => {
                if let Some(ann) = type_ann
                    && let Some(declared) = resolve_struct_ann(ann.trim(), env)
                    && matches!(ast.get_expr(*init), Expr::ObjectLit { .. })
                {
                    push_objlit_jobs(ast, *init, declared, env, jobs);
                }
                collect_expr_jobs(ast, *init, env, fn_params, jobs);
            }
            Stmt::FnDecl { body, .. } => collect_jobs(ast, body, env, fn_params, jobs),
            Stmt::Block(inner) | Stmt::Multi(inner) => {
                collect_jobs(ast, inner, env, fn_params, jobs)
            }
            Stmt::Expr(eid) | Stmt::Return(Some(eid)) | Stmt::Throw(eid) | Stmt::Yield(eid) => {
                collect_expr_jobs(ast, *eid, env, fn_params, jobs);
            }
            Stmt::If {
                cond,
                then_branch,
                else_branch,
            } => {
                collect_expr_jobs(ast, *cond, env, fn_params, jobs);
                collect_jobs(
                    ast,
                    core::slice::from_ref(then_branch.as_ref()),
                    env,
                    fn_params,
                    jobs,
                );
                if let Some(eb) = else_branch {
                    collect_jobs(
                        ast,
                        core::slice::from_ref(eb.as_ref()),
                        env,
                        fn_params,
                        jobs,
                    );
                }
            }
            Stmt::While { cond, body } | Stmt::DoWhile { body, cond } => {
                collect_expr_jobs(ast, *cond, env, fn_params, jobs);
                collect_jobs(
                    ast,
                    core::slice::from_ref(body.as_ref()),
                    env,
                    fn_params,
                    jobs,
                );
            }
            Stmt::ForOf { body, .. } | Stmt::Labeled { body, .. } => {
                collect_jobs(
                    ast,
                    core::slice::from_ref(body.as_ref()),
                    env,
                    fn_params,
                    jobs,
                );
            }
            Stmt::For {
                init,
                cond,
                step,
                body,
            } => {
                if let Some(i) = init {
                    collect_jobs(ast, core::slice::from_ref(i.as_ref()), env, fn_params, jobs);
                }
                for e in [cond, step].into_iter().flatten() {
                    collect_expr_jobs(ast, *e, env, fn_params, jobs);
                }
                collect_jobs(
                    ast,
                    core::slice::from_ref(body.as_ref()),
                    env,
                    fn_params,
                    jobs,
                );
            }
            Stmt::Switch {
                scrutinee,
                cases,
                default,
            } => {
                collect_expr_jobs(ast, *scrutinee, env, fn_params, jobs);
                for c in cases {
                    collect_jobs(ast, &c.body, env, fn_params, jobs);
                }
                if let Some(d) = default {
                    collect_jobs(ast, d, env, fn_params, jobs);
                }
            }
            Stmt::Try {
                body,
                catch_body,
                finally_body,
                ..
            } => {
                collect_jobs(ast, body, env, fn_params, jobs);
                collect_jobs(ast, catch_body, env, fn_params, jobs);
                if let Some(fb) = finally_body {
                    collect_jobs(ast, fb, env, fn_params, jobs);
                }
            }
            _ => {}
        }
    }
}

/// Chunk 784 — recursive expression walk finding direct-call
/// argument fill sites: `f({ ... })` where `f` is a top-level FnDecl
/// whose matching param annotation resolves to a declared field
/// list. Containers recurse so nested calls (`console.log(f({}))`)
/// are reached; method calls / `new` args recurse for nested sites
/// but their own params stay unmatched (loud — archived).
fn collect_expr_jobs(
    ast: &Ast,
    eid: ExprId,
    env: &AliasEnv,
    fn_params: &HashMap<String, Vec<Option<String>>>,
    jobs: &mut Vec<(ExprId, Vec<(PropKey, String)>)>,
) {
    match ast.get_expr(eid) {
        Expr::Call { callee, args } => {
            if let Expr::Ident(cname) = ast.get_expr(*callee)
                && let Some(params) = fn_params.get(cname)
            {
                for (i, arg) in args.iter().enumerate() {
                    if let Some(Some(ann)) = params.get(i)
                        && let Some(declared) = resolve_struct_ann(ann.trim(), env)
                        && matches!(ast.get_expr(*arg), Expr::ObjectLit { .. })
                    {
                        push_objlit_jobs(ast, *arg, declared, env, jobs);
                    }
                }
            }
            collect_expr_jobs(ast, *callee, env, fn_params, jobs);
            for a in args {
                collect_expr_jobs(ast, *a, env, fn_params, jobs);
            }
        }
        Expr::ObjectLit { fields } => {
            for (_, feid) in fields {
                collect_expr_jobs(ast, *feid, env, fn_params, jobs);
            }
        }
        Expr::Array(els) => {
            for e in els {
                collect_expr_jobs(ast, *e, env, fn_params, jobs);
            }
        }
        Expr::BinOp { left, right, .. }
        | Expr::Sequence { left, right }
        | Expr::Nullish {
            lhs: left,
            rhs: right,
        } => {
            collect_expr_jobs(ast, *left, env, fn_params, jobs);
            collect_expr_jobs(ast, *right, env, fn_params, jobs);
        }
        Expr::Assign { target, value } => {
            collect_expr_jobs(ast, *target, env, fn_params, jobs);
            collect_expr_jobs(ast, *value, env, fn_params, jobs);
        }
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_expr_jobs(ast, *cond, env, fn_params, jobs);
            collect_expr_jobs(ast, *then_branch, env, fn_params, jobs);
            collect_expr_jobs(ast, *else_branch, env, fn_params, jobs);
        }
        Expr::Unary { expr, .. }
        | Expr::TypeOf { expr }
        | Expr::Spread { expr }
        | Expr::As { expr, .. } => {
            collect_expr_jobs(ast, *expr, env, fn_params, jobs);
        }
        Expr::InstanceOf { expr, rhs } => {
            collect_expr_jobs(ast, *expr, env, fn_params, jobs);
            collect_expr_jobs(ast, *rhs, env, fn_params, jobs);
        }
        Expr::PostIncr { target, .. } => {
            collect_expr_jobs(ast, *target, env, fn_params, jobs);
        }
        Expr::Member { obj, .. } | Expr::OptChain { obj, .. } => {
            collect_expr_jobs(ast, *obj, env, fn_params, jobs);
        }
        Expr::Index { obj, index } | Expr::OptIndex { obj, index } => {
            collect_expr_jobs(ast, *obj, env, fn_params, jobs);
            collect_expr_jobs(ast, *index, env, fn_params, jobs);
        }
        Expr::OptCall { callee, args } => {
            collect_expr_jobs(ast, *callee, env, fn_params, jobs);
            for a in args {
                collect_expr_jobs(ast, *a, env, fn_params, jobs);
            }
        }
        Expr::New { args, .. } | Expr::Super { args } => {
            for a in args {
                collect_expr_jobs(ast, *a, env, fn_params, jobs);
            }
        }
        _ => {}
    }
}
