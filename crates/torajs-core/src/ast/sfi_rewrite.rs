//! T-22 Stmt::ForOfSplitIter — body rewriters that fold split-iter
//! `i` / `v` synthesis sites into the existing user-named `x` binding.
//!
//! Chunk 347 — extracted from ast.rs. Caller path
//! `crate::ast::sfi_rewrite::sfi_rewrite_body` (the wrapper) is
//! used by the SFI desugar pass elsewhere in ast.rs (line ~5231).
//! The two recursive walkers (`sfi_rewrite_stmt` + `sfi_rewrite_expr`)
//! stay sibling-private.

use super::{Ast, Expr, ExprId, Stmt, SwitchCase};

/// X-references match this exact shape.
pub(super) fn sfi_rewrite_body(
    ast: &mut Ast,
    body: &Stmt,
    x_name: &str,
    i_name: &str,
    v_name: &str,
) -> Stmt {
    sfi_rewrite_stmt(ast, body, x_name, i_name, v_name)
}

fn sfi_rewrite_stmt(ast: &mut Ast, s: &Stmt, x_name: &str, i_name: &str, v_name: &str) -> Stmt {
    match s {
        Stmt::Expr(eid) => Stmt::Expr(sfi_rewrite_expr(ast, *eid, x_name, i_name, v_name)),
        Stmt::Throw(eid) => Stmt::Throw(sfi_rewrite_expr(ast, *eid, x_name, i_name, v_name)),
        Stmt::Yield(eid) => Stmt::Yield(sfi_rewrite_expr(ast, *eid, x_name, i_name, v_name)),
        Stmt::YieldInto {
            var,
            type_ann,
            value,
        } => Stmt::YieldInto {
            var: var.clone(),
            type_ann: type_ann.clone(),
            value: sfi_rewrite_expr(ast, *value, x_name, i_name, v_name),
        },
        Stmt::Return(Some(eid)) => {
            Stmt::Return(Some(sfi_rewrite_expr(ast, *eid, x_name, i_name, v_name)))
        }
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
            init: sfi_rewrite_expr(ast, *init, x_name, i_name, v_name),
            // preserve the `var` flag through the rewrite — hardcoding
            // false silently dropped var-hoist semantics on any
            // rewritten `var` decl (surfaced by the zero-warn rule).
            is_var: *is_var,
        },
        Stmt::UsingDecl {
            name,
            type_ann,
            init,
        } => Stmt::UsingDecl {
            name: name.clone(),
            type_ann: type_ann.clone(),
            init: sfi_rewrite_expr(ast, *init, x_name, i_name, v_name),
        },
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => Stmt::If {
            cond: sfi_rewrite_expr(ast, *cond, x_name, i_name, v_name),
            then_branch: Box::new(sfi_rewrite_stmt(ast, then_branch, x_name, i_name, v_name)),
            else_branch: else_branch
                .as_ref()
                .map(|eb| Box::new(sfi_rewrite_stmt(ast, eb, x_name, i_name, v_name))),
        },
        Stmt::While { cond, body } => Stmt::While {
            cond: sfi_rewrite_expr(ast, *cond, x_name, i_name, v_name),
            body: Box::new(sfi_rewrite_stmt(ast, body, x_name, i_name, v_name)),
        },
        Stmt::DoWhile { body, cond } => Stmt::DoWhile {
            body: Box::new(sfi_rewrite_stmt(ast, body, x_name, i_name, v_name)),
            cond: sfi_rewrite_expr(ast, *cond, x_name, i_name, v_name),
        },
        Stmt::Switch {
            scrutinee,
            cases,
            default,
        } => Stmt::Switch {
            scrutinee: sfi_rewrite_expr(ast, *scrutinee, x_name, i_name, v_name),
            cases: cases
                .iter()
                .map(|c| SwitchCase {
                    value: sfi_rewrite_expr(ast, c.value, x_name, i_name, v_name),
                    body: c
                        .body
                        .iter()
                        .map(|s| sfi_rewrite_stmt(ast, s, x_name, i_name, v_name))
                        .collect(),
                })
                .collect(),
            default: default.as_ref().map(|db| {
                db.iter()
                    .map(|s| sfi_rewrite_stmt(ast, s, x_name, i_name, v_name))
                    .collect()
            }),
        },
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => Stmt::For {
            init: init
                .as_ref()
                .map(|i| Box::new(sfi_rewrite_stmt(ast, i, x_name, i_name, v_name))),
            cond: cond.map(|c| sfi_rewrite_expr(ast, c, x_name, i_name, v_name)),
            step: step.map(|st| sfi_rewrite_expr(ast, st, x_name, i_name, v_name)),
            body: Box::new(sfi_rewrite_stmt(ast, body, x_name, i_name, v_name)),
        },
        Stmt::ForOfSplitIter {
            var_name,
            parent,
            sep,
            body,
        } => Stmt::ForOfSplitIter {
            var_name: var_name.clone(),
            parent: sfi_rewrite_expr(ast, *parent, x_name, i_name, v_name),
            sep: sfi_rewrite_expr(ast, *sep, x_name, i_name, v_name),
            body: Box::new(sfi_rewrite_stmt(ast, body, x_name, i_name, v_name)),
        },
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
            elem_expr: sfi_rewrite_expr(ast, *elem_expr, x_name, i_name, v_name),
            body: Box::new(sfi_rewrite_stmt(ast, body, x_name, i_name, v_name)),
            forin_obj: *forin_obj,
            is_await: *is_await,
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
                .map(|s| sfi_rewrite_stmt(ast, s, x_name, i_name, v_name))
                .collect(),
            had_catch: *had_catch,
            catch_param: catch_param.clone(),
            catch_type: catch_type.clone(),
            catch_body: catch_body
                .iter()
                .map(|s| sfi_rewrite_stmt(ast, s, x_name, i_name, v_name))
                .collect(),
            finally_body: finally_body.as_ref().map(|fb| {
                fb.iter()
                    .map(|s| sfi_rewrite_stmt(ast, s, x_name, i_name, v_name))
                    .collect()
            }),
        },
        Stmt::Block(stmts) => Stmt::Block(
            stmts
                .iter()
                .map(|s| sfi_rewrite_stmt(ast, s, x_name, i_name, v_name))
                .collect(),
        ),
        Stmt::Multi(stmts) => Stmt::Multi(
            stmts
                .iter()
                .map(|s| sfi_rewrite_stmt(ast, s, x_name, i_name, v_name))
                .collect(),
        ),
        Stmt::Break(_) | Stmt::Continue(_) => s.clone(),
        Stmt::Labeled { label, body } => Stmt::Labeled {
            label: label.clone(),
            body: Box::new(sfi_rewrite_stmt(ast, body, x_name, i_name, v_name)),
        },
        Stmt::FnDecl { .. }
        | Stmt::TypeDecl { .. }
        | Stmt::ClassDecl { .. }
        | Stmt::ImportDecl { .. }
        | Stmt::ExportDecl { .. } => s.clone(),
    }
}

fn sfi_rewrite_expr(
    ast: &mut Ast,
    eid: ExprId,
    x_name: &str,
    i_name: &str,
    v_name: &str,
) -> ExprId {
    let cur = ast.get_expr(eid).clone();
    match cur {
        Expr::Index { obj, index } => {
            // X[I] → Ident(v_name). Otherwise descend.
            if let Expr::Ident(n) = ast.get_expr(obj).clone()
                && n == x_name
                && let Expr::Ident(in_) = ast.get_expr(index).clone()
                && in_ == i_name
            {
                return ast.add_expr(Expr::Ident(v_name.to_string()));
            }
            let new_obj = sfi_rewrite_expr(ast, obj, x_name, i_name, v_name);
            let new_index = sfi_rewrite_expr(ast, index, x_name, i_name, v_name);
            ast.add_expr(Expr::Index {
                obj: new_obj,
                index: new_index,
            })
        }
        Expr::Member { obj, name } => {
            let new_obj = sfi_rewrite_expr(ast, obj, x_name, i_name, v_name);
            ast.add_expr(Expr::Member { obj: new_obj, name })
        }
        Expr::Call { callee, args } => {
            let new_callee = sfi_rewrite_expr(ast, callee, x_name, i_name, v_name);
            let new_args: Vec<ExprId> = args
                .iter()
                .map(|a| sfi_rewrite_expr(ast, *a, x_name, i_name, v_name))
                .collect();
            ast.add_expr(Expr::Call {
                callee: new_callee,
                args: new_args,
            })
        }
        Expr::BinOp { op, left, right } => {
            let l = sfi_rewrite_expr(ast, left, x_name, i_name, v_name);
            let r = sfi_rewrite_expr(ast, right, x_name, i_name, v_name);
            ast.add_expr(Expr::BinOp {
                op,
                left: l,
                right: r,
            })
        }
        Expr::Assign { target, value } => {
            let t = sfi_rewrite_expr(ast, target, x_name, i_name, v_name);
            let v = sfi_rewrite_expr(ast, value, x_name, i_name, v_name);
            ast.add_expr(Expr::Assign {
                target: t,
                value: v,
            })
        }
        Expr::Unary { op, expr } => {
            let e = sfi_rewrite_expr(ast, expr, x_name, i_name, v_name);
            ast.add_expr(Expr::Unary { op, expr: e })
        }
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            let c = sfi_rewrite_expr(ast, cond, x_name, i_name, v_name);
            let t = sfi_rewrite_expr(ast, then_branch, x_name, i_name, v_name);
            let e = sfi_rewrite_expr(ast, else_branch, x_name, i_name, v_name);
            ast.add_expr(Expr::Ternary {
                cond: c,
                then_branch: t,
                else_branch: e,
            })
        }
        Expr::Array(els) => {
            let new_els: Vec<ExprId> = els
                .iter()
                .map(|e| sfi_rewrite_expr(ast, *e, x_name, i_name, v_name))
                .collect();
            ast.add_expr(Expr::Array(new_els))
        }
        Expr::Spread { expr } => {
            let e = sfi_rewrite_expr(ast, expr, x_name, i_name, v_name);
            ast.add_expr(Expr::Spread { expr: e })
        }
        Expr::ObjectLit { fields } => {
            let new_fields: Vec<(String, ExprId)> = fields
                .iter()
                .map(|(n, e)| (n.clone(), sfi_rewrite_expr(ast, *e, x_name, i_name, v_name)))
                .collect();
            ast.add_expr(Expr::ObjectLit { fields: new_fields })
        }
        Expr::PostIncr { target, is_inc } => {
            let t = sfi_rewrite_expr(ast, target, x_name, i_name, v_name);
            ast.add_expr(Expr::PostIncr { target: t, is_inc })
        }
        Expr::OptChain { obj, name } => {
            let o = sfi_rewrite_expr(ast, obj, x_name, i_name, v_name);
            ast.add_expr(Expr::OptChain { obj: o, name })
        }
        Expr::OptIndex { obj, index } => {
            let o = sfi_rewrite_expr(ast, obj, x_name, i_name, v_name);
            let i = sfi_rewrite_expr(ast, index, x_name, i_name, v_name);
            ast.add_expr(Expr::OptIndex { obj: o, index: i })
        }
        Expr::OptCall { callee, args } => {
            let c = sfi_rewrite_expr(ast, callee, x_name, i_name, v_name);
            let new_args: Vec<ExprId> = args
                .clone()
                .into_iter()
                .map(|a| sfi_rewrite_expr(ast, a, x_name, i_name, v_name))
                .collect();
            ast.add_expr(Expr::OptCall {
                callee: c,
                args: new_args,
            })
        }
        Expr::Nullish { lhs, rhs } => {
            let l = sfi_rewrite_expr(ast, lhs, x_name, i_name, v_name);
            let r = sfi_rewrite_expr(ast, rhs, x_name, i_name, v_name);
            ast.add_expr(Expr::Nullish { lhs: l, rhs: r })
        }
        Expr::New {
            class_name,
            args,
            type_args,
        } => {
            let new_args: Vec<ExprId> = args
                .iter()
                .map(|a| sfi_rewrite_expr(ast, *a, x_name, i_name, v_name))
                .collect();
            ast.add_expr(Expr::New {
                class_name,
                args: new_args,
                type_args,
            })
        }
        // Leaves and shapes that don't carry X-referencing children
        // (Ident / Number / String / Bool / Null / closures / etc) — clone.
        _ => eid,
    }
}
