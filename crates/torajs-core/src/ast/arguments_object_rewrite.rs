//! T-11 / T-31 — `arguments` Ident → params / `__torajs_arguments`
//! rewriter for desugar_arguments_object.
//!
//! Chunk 344 — extracted from ast.rs. The two recursive rewriters
//! (`rewrite_arguments_in_stmt` walks every statement form,
//! `rewrite_arguments_in_expr` walks every Expr variant, and they
//! call each other on Expr inside Stmt) form one logical unit and
//! lift cleanly into a single sibling. Caller
//! (`ast::desugar_arguments_object`) reaches them via `super::`
//! and the pub(super) markers below.

use super::{Ast, Expr, ExprId, Stmt};

pub(super) fn rewrite_arguments_in_stmt(
    ast: &mut Ast,
    s: &Stmt,
    params: &[String],
    uses_real_argc: bool,
) -> Stmt {
    match s {
        Stmt::Expr(eid) => Stmt::Expr(rewrite_arguments_in_expr(ast, *eid, params, uses_real_argc)),
        Stmt::Throw(eid) => {
            Stmt::Throw(rewrite_arguments_in_expr(ast, *eid, params, uses_real_argc))
        }
        Stmt::Return(Some(eid)) => Stmt::Return(Some(rewrite_arguments_in_expr(
            ast,
            *eid,
            params,
            uses_real_argc,
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
            init: rewrite_arguments_in_expr(ast, *init, params, uses_real_argc),
            // preserve `var` flag (hardcoded false dropped var-hoist
            // semantics on rewritten `var` decls; zero-warn surfaced it).
            is_var: *is_var,
        },
        Stmt::Block(stmts) => Stmt::Block(
            stmts
                .iter()
                .map(|s| rewrite_arguments_in_stmt(ast, s, params, uses_real_argc))
                .collect(),
        ),
        Stmt::Multi(stmts) => Stmt::Multi(
            stmts
                .iter()
                .map(|s| rewrite_arguments_in_stmt(ast, s, params, uses_real_argc))
                .collect(),
        ),
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => Stmt::If {
            cond: rewrite_arguments_in_expr(ast, *cond, params, uses_real_argc),
            then_branch: Box::new(rewrite_arguments_in_stmt(
                ast,
                then_branch,
                params,
                uses_real_argc,
            )),
            else_branch: else_branch
                .as_ref()
                .map(|eb| Box::new(rewrite_arguments_in_stmt(ast, eb, params, uses_real_argc))),
        },
        Stmt::While { cond, body } => Stmt::While {
            cond: rewrite_arguments_in_expr(ast, *cond, params, uses_real_argc),
            body: Box::new(rewrite_arguments_in_stmt(ast, body, params, uses_real_argc)),
        },
        Stmt::DoWhile { cond, body } => Stmt::DoWhile {
            cond: rewrite_arguments_in_expr(ast, *cond, params, uses_real_argc),
            body: Box::new(rewrite_arguments_in_stmt(ast, body, params, uses_real_argc)),
        },
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => Stmt::For {
            init: init
                .as_ref()
                .map(|i| Box::new(rewrite_arguments_in_stmt(ast, i, params, uses_real_argc))),
            cond: cond.map(|c| rewrite_arguments_in_expr(ast, c, params, uses_real_argc)),
            step: step.map(|u| rewrite_arguments_in_expr(ast, u, params, uses_real_argc)),
            body: Box::new(rewrite_arguments_in_stmt(ast, body, params, uses_real_argc)),
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
                .map(|s| rewrite_arguments_in_stmt(ast, s, params, uses_real_argc))
                .collect(),
            had_catch: *had_catch,
            catch_param: catch_param.clone(),
            catch_type: catch_type.clone(),
            catch_body: catch_body
                .iter()
                .map(|s| rewrite_arguments_in_stmt(ast, s, params, uses_real_argc))
                .collect(),
            finally_body: finally_body.as_ref().map(|fb| {
                fb.iter()
                    .map(|s| rewrite_arguments_in_stmt(ast, s, params, uses_real_argc))
                    .collect()
            }),
        },
        // Nested FnDecl owns its own arguments scope — leave it for
        // the outer pass to handle independently when it iterates
        // ast.stmts (lift_arrow_fns has already hoisted closures to
        // top-level FnDecls, so nested-FnDecl-in-body is rare in
        // practice).
        other => other.clone(),
    }
}

pub(super) fn rewrite_arguments_in_expr(
    ast: &mut Ast,
    eid: ExprId,
    params: &[String],
    uses_real_argc: bool,
) -> ExprId {
    let e = ast.get_expr(eid).clone();
    match e {
        // `arguments.length` — T-31: when this fn uses real argc,
        // route to `Ident("__torajs_real_argc")` (the synthetic param
        // injected by `desugar_arguments_object`). Otherwise fall back
        // to the declared-arity fold (`Number(<arity>)`) — that path
        // still serves closures and class methods that don't qualify
        // for the T-31 ABI change.
        Expr::Member { obj, name } if name == "length" => {
            if let Expr::Ident(n) = ast.get_expr(obj)
                && n == "arguments"
            {
                if uses_real_argc {
                    return ast.add_expr(Expr::Ident("__torajs_real_argc".into()));
                }
                return ast.add_expr(Expr::Number(params.len() as f64));
            }
            // Recurse through the receiver; non-arguments member access
            // gets a fresh node so nested rewrites still reach the
            // children.
            let new_obj = rewrite_arguments_in_expr(ast, obj, params, uses_real_argc);
            ast.add_expr(Expr::Member { obj: new_obj, name })
        }
        // `arguments[N]` with literal N in [0, arity) → Ident(param[N]).
        // T-11 — `arguments[<non-literal>]` (or out-of-range literal)
        // → `__torajs_arguments[<i>]` reading from the synthesized
        // Array<Any>. The synth let is prepended at fn body start by
        // the FnDecl-walk pre-pass when any dynamic use is detected.
        Expr::Index { obj, index } => {
            let is_arguments = matches!(
                ast.get_expr(obj),
                Expr::Ident(n) if n == "arguments"
            );
            if is_arguments {
                if let Expr::Number(n) = ast.get_expr(index)
                    && n.fract() == 0.0
                    && (*n as usize) < params.len()
                {
                    let pname = params[*n as usize].clone();
                    return ast.add_expr(Expr::Ident(pname));
                }
                // Dynamic index (or out-of-range literal): route to
                // the materialized Array<Any> via __torajs_arguments.
                let new_index = rewrite_arguments_in_expr(ast, index, params, uses_real_argc);
                let synth_obj = ast.add_expr(Expr::Ident("__torajs_arguments".into()));
                return ast.add_expr(Expr::Index {
                    obj: synth_obj,
                    index: new_index,
                });
            }
            let new_obj = rewrite_arguments_in_expr(ast, obj, params, uses_real_argc);
            let new_index = rewrite_arguments_in_expr(ast, index, params, uses_real_argc);
            ast.add_expr(Expr::Index {
                obj: new_obj,
                index: new_index,
            })
        }
        Expr::BinOp { op, left, right } => {
            let l = rewrite_arguments_in_expr(ast, left, params, uses_real_argc);
            let r = rewrite_arguments_in_expr(ast, right, params, uses_real_argc);
            ast.add_expr(Expr::BinOp {
                op,
                left: l,
                right: r,
            })
        }
        Expr::Unary { op, expr } => {
            let e2 = rewrite_arguments_in_expr(ast, expr, params, uses_real_argc);
            ast.add_expr(Expr::Unary { op, expr: e2 })
        }
        Expr::Call { callee, args } => {
            let c = rewrite_arguments_in_expr(ast, callee, params, uses_real_argc);
            /* `f(...arguments)` — expand the spread inline into the
             * call arg list as `f(p0, p1, ...)`. Handles arbitrary
             * mix of regular args and the spread. */
            let mut new_args: Vec<ExprId> = Vec::with_capacity(args.len());
            for a in &args {
                if let Expr::Spread { expr } = ast.get_expr(*a)
                    && let Expr::Ident(n) = ast.get_expr(*expr)
                    && n == "arguments"
                {
                    for p in params {
                        new_args.push(ast.add_expr(Expr::Ident(p.clone())));
                    }
                    continue;
                }
                new_args.push(rewrite_arguments_in_expr(ast, *a, params, uses_real_argc));
            }
            ast.add_expr(Expr::Call {
                callee: c,
                args: new_args,
            })
        }
        Expr::Member { obj, name } => {
            let o = rewrite_arguments_in_expr(ast, obj, params, uses_real_argc);
            ast.add_expr(Expr::Member { obj: o, name })
        }
        Expr::Assign { target, value } => {
            let t = rewrite_arguments_in_expr(ast, target, params, uses_real_argc);
            let v = rewrite_arguments_in_expr(ast, value, params, uses_real_argc);
            ast.add_expr(Expr::Assign {
                target: t,
                value: v,
            })
        }
        Expr::Array(elems) => {
            /* `[...arguments]` — expand the spread inline. Same shape
             * as the Call arm above. Mixed elems (regular + spread)
             * supported by interleaving. */
            let mut new_elems: Vec<ExprId> = Vec::with_capacity(elems.len());
            for e in &elems {
                if let Expr::Spread { expr } = ast.get_expr(*e)
                    && let Expr::Ident(n) = ast.get_expr(*expr)
                    && n == "arguments"
                {
                    for p in params {
                        new_elems.push(ast.add_expr(Expr::Ident(p.clone())));
                    }
                    continue;
                }
                new_elems.push(rewrite_arguments_in_expr(ast, *e, params, uses_real_argc));
            }
            ast.add_expr(Expr::Array(new_elems))
        }
        Expr::ObjectLit { fields } => {
            let new_fields: Vec<(String, ExprId)> = fields
                .iter()
                .map(|(n, e)| {
                    (
                        n.clone(),
                        rewrite_arguments_in_expr(ast, *e, params, uses_real_argc),
                    )
                })
                .collect();
            ast.add_expr(Expr::ObjectLit { fields: new_fields })
        }
        // Leaf / opaque shapes — no children to recurse through here.
        // Intentionally returns the original `eid` so we don't bloat
        // the arena with no-op clones.
        _ => eid,
    }
}
