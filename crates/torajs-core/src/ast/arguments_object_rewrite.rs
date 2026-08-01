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

use super::arguments_object::ArgcMode;
use super::arguments_object_rewrite_spread::{rewrite_array_arm, rewrite_call_arm};
use super::{Ast, Expr, ExprId, Stmt};

pub(super) fn rewrite_arguments_in_stmt(
    ast: &mut Ast,
    s: &Stmt,
    params: &[String],
    argc_mode: ArgcMode,
    is_argv_fn: bool,
) -> Stmt {
    match s {
        Stmt::Expr(eid) => Stmt::Expr(rewrite_arguments_in_expr(
            ast, *eid, params, argc_mode, is_argv_fn,
        )),
        Stmt::Throw(eid) => Stmt::Throw(rewrite_arguments_in_expr(
            ast, *eid, params, argc_mode, is_argv_fn,
        )),
        Stmt::Return(Some(eid)) => Stmt::Return(Some(rewrite_arguments_in_expr(
            ast, *eid, params, argc_mode, is_argv_fn,
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
            init: rewrite_arguments_in_expr(ast, *init, params, argc_mode, is_argv_fn),
            // preserve `var` flag (hardcoded false dropped var-hoist
            // semantics on rewritten `var` decls; zero-warn surfaced it).
            is_var: *is_var,
        },
        Stmt::Block(stmts) => Stmt::Block(
            stmts
                .iter()
                .map(|s| rewrite_arguments_in_stmt(ast, s, params, argc_mode, is_argv_fn))
                .collect(),
        ),
        Stmt::Multi(stmts) => Stmt::Multi(
            stmts
                .iter()
                .map(|s| rewrite_arguments_in_stmt(ast, s, params, argc_mode, is_argv_fn))
                .collect(),
        ),
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => Stmt::If {
            cond: rewrite_arguments_in_expr(ast, *cond, params, argc_mode, is_argv_fn),
            then_branch: Box::new(rewrite_arguments_in_stmt(
                ast,
                then_branch,
                params,
                argc_mode,
                is_argv_fn,
            )),
            else_branch: else_branch.as_ref().map(|eb| {
                Box::new(rewrite_arguments_in_stmt(
                    ast, eb, params, argc_mode, is_argv_fn,
                ))
            }),
        },
        Stmt::Labeled { label, body } => Stmt::Labeled {
            label: label.clone(),
            body: Box::new(rewrite_arguments_in_stmt(
                ast, body, params, argc_mode, is_argv_fn,
            )),
        },
        Stmt::While { cond, body } => Stmt::While {
            cond: rewrite_arguments_in_expr(ast, *cond, params, argc_mode, is_argv_fn),
            body: Box::new(rewrite_arguments_in_stmt(
                ast, body, params, argc_mode, is_argv_fn,
            )),
        },
        Stmt::DoWhile { cond, body } => Stmt::DoWhile {
            cond: rewrite_arguments_in_expr(ast, *cond, params, argc_mode, is_argv_fn),
            body: Box::new(rewrite_arguments_in_stmt(
                ast, body, params, argc_mode, is_argv_fn,
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
                    ast, i, params, argc_mode, is_argv_fn,
                ))
            }),
            cond: cond.map(|c| rewrite_arguments_in_expr(ast, c, params, argc_mode, is_argv_fn)),
            step: step.map(|u| rewrite_arguments_in_expr(ast, u, params, argc_mode, is_argv_fn)),
            body: Box::new(rewrite_arguments_in_stmt(
                ast, body, params, argc_mode, is_argv_fn,
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
                .map(|s| rewrite_arguments_in_stmt(ast, s, params, argc_mode, is_argv_fn))
                .collect(),
            had_catch: *had_catch,
            catch_param: catch_param.clone(),
            catch_type: catch_type.clone(),
            catch_body: catch_body
                .iter()
                .map(|s| rewrite_arguments_in_stmt(ast, s, params, argc_mode, is_argv_fn))
                .collect(),
            finally_body: finally_body.as_ref().map(|fb| {
                fb.iter()
                    .map(|s| rewrite_arguments_in_stmt(ast, s, params, argc_mode, is_argv_fn))
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
            elem_expr: rewrite_arguments_in_expr(ast, *elem_expr, params, argc_mode, is_argv_fn),
            body: Box::new(rewrite_arguments_in_stmt(
                ast, body, params, argc_mode, is_argv_fn,
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
            parent: rewrite_arguments_in_expr(ast, *parent, params, argc_mode, is_argv_fn),
            sep: rewrite_arguments_in_expr(ast, *sep, params, argc_mode, is_argv_fn),
            body: Box::new(rewrite_arguments_in_stmt(
                ast, body, params, argc_mode, is_argv_fn,
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

pub(super) fn rewrite_arguments_in_expr(
    ast: &mut Ast,
    eid: ExprId,
    params: &[String],
    argc_mode: ArgcMode,
    is_argv_fn: bool,
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
                match argc_mode {
                    ArgcMode::Real => {
                        return ast.add_expr(Expr::Ident("__torajs_real_argc".into()));
                    }
                    ArgcMode::FoldArity => {
                        return ast.add_expr(Expr::Number(params.len() as f64));
                    }
                    // RFC 20260801 — IIFE static-argv face: the call
                    // site's exact arg count (NOT params.len(), which
                    // over-counts on an under-filled site and carries
                    // the injected extras otherwise). The unmapped
                    // face folds the same count — only element
                    // aliasing differs (see the Index arm).
                    ArgcMode::Unmapped(n) => {
                        return ast.add_expr(Expr::Number(n as f64));
                    }
                    // Length-write knife — reads AND writes ride the
                    // materialized array's live `.length` (this arm
                    // serves both positions: an Assign target and a
                    // PostIncr target flow through it unchanged).
                    ArgcMode::LiveLength(_) => {
                        let synth_obj = ast.add_expr(Expr::Ident("__torajs_arguments".into()));
                        return ast.add_expr(Expr::Member {
                            obj: synth_obj,
                            name,
                        });
                    }
                    // Keep the `arguments.length` node untouched so the
                    // checker rejects it loudly — a closure VALUE's real
                    // argc needs the ABI face (recorded); folding the
                    // declared arity here would be silent-wrong.
                    ArgcMode::KeepLoud => return eid,
                }
            }
            // Recurse through the receiver. Copy-on-write: an
            // unchanged child keeps the original node — ExprId-keyed
            // side tables (speculative_cm_rewrites & co.) stay valid
            // for every subtree this pass didn't actually rewrite.
            let new_obj = rewrite_arguments_in_expr(ast, obj, params, argc_mode, is_argv_fn);
            if new_obj == obj {
                return eid;
            }
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
                // KeepLoud — leave every `arguments[...]` node
                // untouched so the checker rejects the body loudly
                // (RFC 20260708-closure-argv-face chunk 2: the old
                // unconditional rewrite fed a declared-params-only
                // array to bodies whose real argv face was killed,
                // silently answering undefined beyond declared).
                if argc_mode == ArgcMode::KeepLoud {
                    return eid;
                }
                // Unmapped / LiveLength faces (module code is strict,
                // ES §10.4.4.6/7) — `arguments[i]` never aliases the
                // param: the literal-index substitution below would
                // diverge both ways (`arguments[0] = 2` mutating `a`;
                // `a = 99` showing in a later read). Every index
                // rides the materialized array instead.
                if !matches!(argc_mode, ArgcMode::Unmapped(_) | ArgcMode::LiveLength(_))
                    && let Expr::Number(n) = ast.get_expr(index)
                    && n.fract() == 0.0
                    && (*n as usize) < params.len()
                {
                    let pname = params[*n as usize].clone();
                    return ast.add_expr(Expr::Ident(pname));
                }
                // Dynamic index (or out-of-range literal): route to
                // the materialized Array<Any> via __torajs_arguments.
                let new_index =
                    rewrite_arguments_in_expr(ast, index, params, argc_mode, is_argv_fn);
                let synth_obj = ast.add_expr(Expr::Ident("__torajs_arguments".into()));
                return ast.add_expr(Expr::Index {
                    obj: synth_obj,
                    index: new_index,
                });
            }
            let new_obj = rewrite_arguments_in_expr(ast, obj, params, argc_mode, is_argv_fn);
            let new_index = rewrite_arguments_in_expr(ast, index, params, argc_mode, is_argv_fn);
            if new_obj == obj && new_index == index {
                return eid;
            }
            ast.add_expr(Expr::Index {
                obj: new_obj,
                index: new_index,
            })
        }
        Expr::Call { callee, args } => {
            rewrite_call_arm(ast, eid, callee, args, params, argc_mode, is_argv_fn)
        }
        Expr::Array(elems) => rewrite_array_arm(ast, eid, elems, params, argc_mode, is_argv_fn),
        // RFC 20260801-arguments-escape-face — bare `arguments`
        // escape (return / assign value / call arg / for-of source
        // hoist init): under a materializing mode it becomes the
        // synthesized `__torajs_arguments: any[]` local, which the
        // consumer then treats as an ordinary array-like. Every other
        // mode leaves the node for the checker's loud reject.
        Expr::Ident(n) if n == "arguments" => {
            if is_argv_fn || matches!(argc_mode, ArgcMode::LiveLength(_) | ArgcMode::Unmapped(_)) {
                return ast.add_expr(Expr::Ident("__torajs_arguments".into()));
            }
            eid
        }
        other => rewrite_recurse_arm(ast, eid, other, params, argc_mode, is_argv_fn),
    }
}

/// RFC 20260801 knife 2 — the mechanical copy-on-write recursion
/// arms (walker-mirror family): every shape here mirrors one the
/// walkers' detection scan reaches, so a body the pass decided to
/// materialize gets its touches swapped at any depth — a stale
/// `arguments` ident would silently ride a fallback lane (`typeof
/// arguments` folded to "undefined" through the undeclared-ident
/// typeof fold). Split from [`rewrite_arguments_in_expr`] (length-
/// write knife pushed the match past the 200-line fn limit); the
/// `arguments`-special arms (Member-length / Index / Call / Array /
/// bare Ident) stay in the main fn, everything below only recurses.
fn rewrite_recurse_arm(
    ast: &mut Ast,
    eid: ExprId,
    e: Expr,
    params: &[String],
    argc_mode: ArgcMode,
    is_argv_fn: bool,
) -> ExprId {
    match e {
        Expr::BinOp { op, left, right } => {
            let l = rewrite_arguments_in_expr(ast, left, params, argc_mode, is_argv_fn);
            let r = rewrite_arguments_in_expr(ast, right, params, argc_mode, is_argv_fn);
            if l == left && r == right {
                return eid;
            }
            ast.add_expr(Expr::BinOp {
                op,
                left: l,
                right: r,
            })
        }
        Expr::Unary { op, expr } => {
            let e2 = rewrite_arguments_in_expr(ast, expr, params, argc_mode, is_argv_fn);
            if e2 == expr {
                return eid;
            }
            ast.add_expr(Expr::Unary { op, expr: e2 })
        }
        Expr::TypeOf { expr } => {
            let e2 = rewrite_arguments_in_expr(ast, expr, params, argc_mode, is_argv_fn);
            if e2 == expr {
                return eid;
            }
            ast.add_expr(Expr::TypeOf { expr: e2 })
        }
        // Length-write knife — `arguments.length--` (walker-mirror:
        // the scans reach PostIncr targets; without this arm the
        // stale Member leaked to the lowering as "post-incr field on
        // non-obj Ptr"). Real mode lands on `__torajs_real_argc--`,
        // LiveLength on `__torajs_arguments.length--`.
        Expr::PostIncr { target, is_inc } => {
            let t = rewrite_arguments_in_expr(ast, target, params, argc_mode, is_argv_fn);
            if t == target {
                return eid;
            }
            ast.add_expr(Expr::PostIncr { target: t, is_inc })
        }
        Expr::Spread { expr } => {
            let e2 = rewrite_arguments_in_expr(ast, expr, params, argc_mode, is_argv_fn);
            if e2 == expr {
                return eid;
            }
            ast.add_expr(Expr::Spread { expr: e2 })
        }
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            let c = rewrite_arguments_in_expr(ast, cond, params, argc_mode, is_argv_fn);
            let t = rewrite_arguments_in_expr(ast, then_branch, params, argc_mode, is_argv_fn);
            let e2 = rewrite_arguments_in_expr(ast, else_branch, params, argc_mode, is_argv_fn);
            if c == cond && t == then_branch && e2 == else_branch {
                return eid;
            }
            ast.add_expr(Expr::Ternary {
                cond: c,
                then_branch: t,
                else_branch: e2,
            })
        }
        Expr::Nullish { lhs, rhs } => {
            let l = rewrite_arguments_in_expr(ast, lhs, params, argc_mode, is_argv_fn);
            let r = rewrite_arguments_in_expr(ast, rhs, params, argc_mode, is_argv_fn);
            if l == lhs && r == rhs {
                return eid;
            }
            ast.add_expr(Expr::Nullish { lhs: l, rhs: r })
        }
        Expr::OptChain { obj, name } => {
            let o = rewrite_arguments_in_expr(ast, obj, params, argc_mode, is_argv_fn);
            if o == obj {
                return eid;
            }
            ast.add_expr(Expr::OptChain { obj: o, name })
        }
        Expr::OptIndex { obj, index } => {
            let o = rewrite_arguments_in_expr(ast, obj, params, argc_mode, is_argv_fn);
            let ix = rewrite_arguments_in_expr(ast, index, params, argc_mode, is_argv_fn);
            if o == obj && ix == index {
                return eid;
            }
            ast.add_expr(Expr::OptIndex { obj: o, index: ix })
        }
        Expr::OptCall { callee, args } => {
            let c = rewrite_arguments_in_expr(ast, callee, params, argc_mode, is_argv_fn);
            let new_args: Vec<ExprId> = args
                .iter()
                .map(|a| rewrite_arguments_in_expr(ast, *a, params, argc_mode, is_argv_fn))
                .collect();
            if c == callee && new_args == args {
                return eid;
            }
            ast.add_expr(Expr::OptCall {
                callee: c,
                args: new_args,
            })
        }
        Expr::Member { obj, name } => {
            let o = rewrite_arguments_in_expr(ast, obj, params, argc_mode, is_argv_fn);
            if o == obj {
                return eid;
            }
            ast.add_expr(Expr::Member { obj: o, name })
        }
        Expr::Assign { target, value } => {
            let t = rewrite_arguments_in_expr(ast, target, params, argc_mode, is_argv_fn);
            let v = rewrite_arguments_in_expr(ast, value, params, argc_mode, is_argv_fn);
            if t == target && v == value {
                return eid;
            }
            ast.add_expr(Expr::Assign {
                target: t,
                value: v,
            })
        }
        Expr::ObjectLit { fields } => {
            let new_fields: Vec<(String, ExprId)> = fields
                .iter()
                .map(|(n, e)| {
                    (
                        n.clone(),
                        rewrite_arguments_in_expr(ast, *e, params, argc_mode, is_argv_fn),
                    )
                })
                .collect();
            if new_fields == fields {
                return eid;
            }
            ast.add_expr(Expr::ObjectLit { fields: new_fields })
        }
        // Leaf / opaque shapes — no children to recurse through here.
        // Intentionally returns the original `eid` so we don't bloat
        // the arena with no-op clones.
        _ => eid,
    }
}
