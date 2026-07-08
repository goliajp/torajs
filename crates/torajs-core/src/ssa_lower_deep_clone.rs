//! Deep-clone Stmt/Expr helpers extracted from `ssa_lower.rs` chunk 365.
//!
//! Used exclusively by generic monomorphization
//! (`ssa_lower_generics_monomorph::monomorphize_generics`) — each mono
//! instantiation gets a private copy of the body's expressions so that
//! subsequent per-instantiation rewrites (substitute_in_stmt +
//! rewrite_tvdefault_in_stmt + rewrite_inner_generic_calls) do not clobber
//! each other. The clone allocates fresh ExprIds by appending to
//! `Ast::exprs`.

use crate::ast::{Ast, Expr, ExprId, Stmt};

/// Deep-clone a Stmt's expression graph into the AST's arena, returning
/// a Stmt that references freshly-allocated ExprIds. Used by
/// monomorphization so each instantiation gets its own private copy of
/// the body's expressions (no shared rewriting between instantiations).
pub(crate) fn deep_clone_stmt(ast: &mut Ast, s: &Stmt) -> Stmt {
    match s {
        Stmt::Expr(eid) => Stmt::Expr(deep_clone_expr(ast, *eid)),
        Stmt::Throw(eid) => Stmt::Throw(deep_clone_expr(ast, *eid)),
        Stmt::Return(maybe) => Stmt::Return(maybe.map(|eid| deep_clone_expr(ast, eid))),
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
            init: deep_clone_expr(ast, *init),
            // a deep clone must preserve `is_var` — hardcoding false
            // silently turned cloned `var` decls into `let`/`const`,
            // dropping var-hoist semantics (zero-warn surfaced it).
            is_var: *is_var,
        },
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => Stmt::If {
            cond: deep_clone_expr(ast, *cond),
            then_branch: Box::new(deep_clone_stmt(ast, then_branch)),
            else_branch: else_branch
                .as_ref()
                .map(|e| Box::new(deep_clone_stmt(ast, e))),
        },
        Stmt::While { cond, body } => Stmt::While {
            cond: deep_clone_expr(ast, *cond),
            body: Box::new(deep_clone_stmt(ast, body)),
        },
        Stmt::DoWhile { body, cond } => Stmt::DoWhile {
            body: Box::new(deep_clone_stmt(ast, body)),
            cond: deep_clone_expr(ast, *cond),
        },
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => Stmt::For {
            init: init.as_ref().map(|i| Box::new(deep_clone_stmt(ast, i))),
            cond: cond.map(|c| deep_clone_expr(ast, c)),
            step: step.map(|s2| deep_clone_expr(ast, s2)),
            body: Box::new(deep_clone_stmt(ast, body)),
        },
        Stmt::Switch {
            scrutinee,
            cases,
            default,
        } => Stmt::Switch {
            scrutinee: deep_clone_expr(ast, *scrutinee),
            cases: cases
                .iter()
                .map(|c| crate::ast::SwitchCase {
                    value: deep_clone_expr(ast, c.value),
                    body: c.body.iter().map(|s| deep_clone_stmt(ast, s)).collect(),
                })
                .collect(),
            default: default
                .as_ref()
                .map(|db| db.iter().map(|s| deep_clone_stmt(ast, s)).collect()),
        },
        Stmt::Block(stmts) => Stmt::Block(stmts.iter().map(|s| deep_clone_stmt(ast, s)).collect()),
        Stmt::Multi(stmts) => Stmt::Multi(stmts.iter().map(|s| deep_clone_stmt(ast, s)).collect()),
        Stmt::Try {
            body,
            had_catch,
            catch_param,
            catch_type,
            catch_body,
            finally_body,
        } => Stmt::Try {
            body: body.iter().map(|s| deep_clone_stmt(ast, s)).collect(),
            had_catch: *had_catch,
            catch_param: catch_param.clone(),
            catch_type: catch_type.clone(),
            catch_body: catch_body.iter().map(|s| deep_clone_stmt(ast, s)).collect(),
            finally_body: finally_body
                .as_ref()
                .map(|fb| fb.iter().map(|s| deep_clone_stmt(ast, s)).collect()),
        },
        // Stmts that don't carry ExprIds — clone trivially.
        other => other.clone(),
    }
}

pub(crate) fn deep_clone_expr(ast: &mut Ast, eid: ExprId) -> ExprId {
    let new_expr = match ast.get_expr(eid) {
        Expr::Ident(n) => Expr::Ident(n.clone()),
        Expr::String(s) => Expr::String(s.clone()),
        Expr::Number(n) => Expr::Number(*n),
        Expr::BigInt { digits, radix } => Expr::BigInt {
            digits: digits.clone(),
            radix: *radix,
        },
        Expr::Bool(b) => Expr::Bool(*b),
        Expr::Null => Expr::Null,
        Expr::Uninit => Expr::Uninit,
        Expr::Regex { pattern, flags } => Expr::Regex {
            pattern: pattern.clone(),
            flags: flags.clone(),
        },
        Expr::This => Expr::This,
        Expr::NewTarget => Expr::NewTarget,
        Expr::BinOp { op, left, right } => {
            let op = *op;
            let l = *left;
            let r = *right;
            Expr::BinOp {
                op,
                left: deep_clone_expr(ast, l),
                right: deep_clone_expr(ast, r),
            }
        }
        Expr::Unary { op, expr } => {
            let op = *op;
            let e = *expr;
            Expr::Unary {
                op,
                expr: deep_clone_expr(ast, e),
            }
        }
        Expr::Member { obj, name } => {
            let o = *obj;
            let name = name.clone();
            Expr::Member {
                obj: deep_clone_expr(ast, o),
                name,
            }
        }
        Expr::Call { callee, args } => {
            let c = *callee;
            let args = args.clone();
            Expr::Call {
                callee: deep_clone_expr(ast, c),
                args: args.into_iter().map(|a| deep_clone_expr(ast, a)).collect(),
            }
        }
        Expr::Assign { target, value } => {
            let t = *target;
            let v = *value;
            Expr::Assign {
                target: deep_clone_expr(ast, t),
                value: deep_clone_expr(ast, v),
            }
        }
        Expr::Index { obj, index } => {
            let o = *obj;
            let i = *index;
            Expr::Index {
                obj: deep_clone_expr(ast, o),
                index: deep_clone_expr(ast, i),
            }
        }
        Expr::Array(els) => {
            let els = els.clone();
            Expr::Array(els.into_iter().map(|e| deep_clone_expr(ast, e)).collect())
        }
        Expr::ObjectLit { fields } => {
            let fields = fields.clone();
            Expr::ObjectLit {
                fields: fields
                    .into_iter()
                    .map(|(n, e)| (n, deep_clone_expr(ast, e)))
                    .collect(),
            }
        }
        Expr::ArrowFn {
            params,
            return_type,
            body,
        } => {
            let params = params.clone();
            let return_type = return_type.clone();
            let body: Vec<Stmt> = body.iter().map(|s| s.clone()).collect();
            // Arrow fn body stmts may carry ExprIds — but at this point
            // arrows are already lifted by lift_arrow_fns in normal pipeline.
            // Defensive: deep-clone each stmt.
            Expr::ArrowFn {
                params,
                return_type,
                body: body.iter().map(|s| deep_clone_stmt(ast, s)).collect(),
            }
        }
        Expr::Closure { fn_name, captures } => Expr::Closure {
            fn_name: fn_name.clone(),
            captures: captures.clone(),
        },
        Expr::New { class_name, args } => {
            let class_name = class_name.clone();
            let args = args.clone();
            Expr::New {
                class_name,
                args: args.into_iter().map(|a| deep_clone_expr(ast, a)).collect(),
            }
        }
        Expr::Super { args } => {
            let args = args.clone();
            Expr::Super {
                args: args.into_iter().map(|a| deep_clone_expr(ast, a)).collect(),
            }
        }
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            let c = *cond;
            let t = *then_branch;
            let e = *else_branch;
            Expr::Ternary {
                cond: deep_clone_expr(ast, c),
                then_branch: deep_clone_expr(ast, t),
                else_branch: deep_clone_expr(ast, e),
            }
        }
        Expr::TypeOf { expr } => {
            let e = *expr;
            Expr::TypeOf {
                expr: deep_clone_expr(ast, e),
            }
        }
        Expr::InstanceOf { expr, class_name } => {
            let e = *expr;
            let cn = class_name.clone();
            Expr::InstanceOf {
                expr: deep_clone_expr(ast, e),
                class_name: cn,
            }
        }
        Expr::Spread { expr } => {
            let e = *expr;
            Expr::Spread {
                expr: deep_clone_expr(ast, e),
            }
        }
        Expr::Nullish { lhs, rhs } => {
            let l = *lhs;
            let r = *rhs;
            Expr::Nullish {
                lhs: deep_clone_expr(ast, l),
                rhs: deep_clone_expr(ast, r),
            }
        }
        Expr::OptChain { obj, name } => {
            let o = *obj;
            let name = name.clone();
            Expr::OptChain {
                obj: deep_clone_expr(ast, o),
                name,
            }
        }
        Expr::OptIndex { obj, index } => {
            let (o, i) = (*obj, *index);
            Expr::OptIndex {
                obj: deep_clone_expr(ast, o),
                index: deep_clone_expr(ast, i),
            }
        }
        Expr::PostIncr { target, is_inc } => {
            let t = *target;
            let is_inc = *is_inc;
            Expr::PostIncr {
                target: deep_clone_expr(ast, t),
                is_inc,
            }
        }
        Expr::As { expr, ty_ann } => {
            let e = *expr;
            let ty_ann = ty_ann.clone();
            Expr::As {
                expr: deep_clone_expr(ast, e),
                ty_ann,
            }
        }
        Expr::Sequence { left, right } => {
            let l = *left;
            let r = *right;
            Expr::Sequence {
                left: deep_clone_expr(ast, l),
                right: deep_clone_expr(ast, r),
            }
        }
    };
    ast.add_expr(new_expr)
}
