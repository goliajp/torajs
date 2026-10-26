//! The statement form the lift gives a guarded node (`&&`, `||`, `??`,
//! a ternary, an optional index or call) whose guarded side awaits.

use super::{LiftLog, replace};
use crate::ast::{Ast, BinOp, Expr, ExprId, Stmt, UnaryOp};

/// The statement form of a guarded node whose guarded side awaits
/// (module doc): `let __awg: any = undefined; if (…) { __awg = … } else
/// { __awg = … }` ahead of the statement, the node reading the temp.
/// The branches still hold their awaits — the caller's rewrite of the
/// prefix turns them into yields.
pub(super) fn guarded_to_statements(
    ast: &mut Ast,
    eid: ExprId,
    prefix: &mut Vec<Stmt>,
    log: &mut LiftLog,
) {
    let tmp = format!("__awg_{}", eid.0);
    let assign_tmp = |ast: &mut Ast, value: ExprId| -> Stmt {
        let target = ast.add_expr(Expr::Ident(tmp.clone()));
        Stmt::Expr(ast.add_expr(Expr::Assign { target, value }))
    };
    let (init, cond, then_v, else_v): (Option<ExprId>, ExprId, ExprId, Option<ExprId>) =
        match ast.get_expr(eid).clone() {
            Expr::Ternary {
                cond,
                then_branch,
                else_branch,
            } => (None, cond, then_branch, Some(else_branch)),
            // `l && r`: g = l; if (g) g = r.   `l || r`: if (!g) g = r.
            // `l ?? r`: if (g == null) g = r.
            Expr::BinOp {
                op: BinOp::LAnd,
                left,
                right,
            } => (
                Some(left),
                ast.add_expr(Expr::Ident(tmp.clone())),
                right,
                None,
            ),
            Expr::BinOp {
                op: BinOp::LOr,
                left,
                right,
            } => {
                let g = ast.add_expr(Expr::Ident(tmp.clone()));
                let not = ast.add_expr(Expr::Unary {
                    op: UnaryOp::Not,
                    expr: g,
                });
                (Some(left), not, right, None)
            }
            Expr::Nullish { lhs, rhs } => {
                let g = ast.add_expr(Expr::Ident(tmp.clone()));
                let null = ast.add_expr(Expr::Null);
                let is_nullish = ast.add_expr(Expr::BinOp {
                    op: BinOp::LooseEq,
                    left: g,
                    right: null,
                });
                (Some(lhs), is_nullish, rhs, None)
            }
            _ => {
                let (cond, then_v) = optional_parts(ast, eid, prefix);
                (None, cond, then_v, None)
            }
        };
    let init = init.unwrap_or_else(|| ast.add_expr(Expr::Ident("undefined".into())));
    prefix.push(Stmt::LetDecl {
        mutable: true,
        name: tmp.clone(),
        type_ann: Some("any".into()),
        init,
        is_var: false,
    });
    let then_s = assign_tmp(ast, then_v);
    let else_s = else_v.map(|v| Box::new(assign_tmp(ast, v)));
    prefix.push(Stmt::If {
        cond,
        then_branch: Box::new(then_s),
        else_branch: else_s,
    });
    replace(ast, eid, Expr::Ident(tmp), log);
}

/// An optional chain past its test (`o?.[k]`, `o?.a.m(x)`, `f?.(x)`, …):
/// its always-evaluated operands, already lifted, bound to temps on
/// `prefix` in order, then the test that the chain does not short-
/// circuit and the node's value past it, spelled without the `?.` it
/// tested (an optional index callee keeps its call's `?.`, which the
/// rewrite of the branch takes in turn). An optional call reads its
/// callee once and calls it on the receiver it was read from.
fn optional_parts(ast: &mut Ast, eid: ExprId, prefix: &mut Vec<Stmt>) -> (ExprId, ExprId) {
    let mut bind = |e: ExprId| -> String {
        let name = format!("__awb_{}", e.0);
        prefix.push(Stmt::LetDecl {
            mutable: false,
            name: name.clone(),
            type_ann: None,
            init: e,
            is_var: false,
        });
        name
    };
    let (callee, args) = match ast.get_expr(eid).clone() {
        Expr::OptCall { callee, args } => (callee, args),
        // a chain through `.name` / `[key]` steps, maybe ending in a call
        node => {
            let (top, args) = match node {
                Expr::Call { callee, args } => (callee, Some(args)),
                _ => (eid, None),
            };
            let b = bind(link_base(ast, top));
            let r = read(ast, &b, None);
            let cond = present(ast, r);
            let value = past_link(ast, top, &b);
            let value = match args {
                Some(args) => ast.add_expr(Expr::Call {
                    callee: value,
                    args,
                }),
                None => value,
            };
            return (cond, value);
        }
    };
    // the callee's value, bound once, and the receiver it was read from
    let (f, this) = match ast.get_expr(callee).clone() {
        Expr::Member { obj, name } => {
            let b = bind(obj);
            let fv = read(ast, &b, Some(Step::Member(name)));
            (bind(fv), Some(b))
        }
        Expr::Index { obj, index } => {
            let b = bind(obj);
            let k = bind(index);
            let kr = read(ast, &k, None);
            let fv = read(ast, &b, Some(Step::Index(kr)));
            (bind(fv), Some(b))
        }
        // `o?.m?.(x)`: a nullish `o` reads as an undefined callee
        Expr::OptChain { obj, name } => {
            let b = bind(obj);
            let r = read(ast, &b, None);
            let fv = ast.add_expr(Expr::OptChain { obj: r, name });
            (bind(fv), Some(b))
        }
        // `o?.[k]?.(x)`: past `o`'s test it is `o[k]?.(x)`, whose key and
        // callee the rewrite of the branch binds in turn
        Expr::OptIndex { obj, .. } => {
            let b = bind(obj);
            let r = read(ast, &b, None);
            let cond = present(ast, r);
            let callee = past_link(ast, callee, &b);
            return (cond, ast.add_expr(Expr::OptCall { callee, args }));
        }
        _ => (bind(callee), None),
    };
    let r = read(ast, &f, None);
    let cond = present(ast, r);
    let fr = read(ast, &f, None);
    let call = match this {
        Some(b) => {
            let this = read(ast, &b, None);
            let list = ast.add_expr(Expr::Array(args));
            let kernel = ast.add_expr(Expr::Ident(CALL_WITH_THIS.into()));
            Expr::Call {
                callee: kernel,
                args: vec![fr, this, list],
            }
        }
        None => Expr::Call { callee: fr, args },
    };
    (cond, ast.add_expr(call))
}

/// `(f, thisValue, [args…])`: the call of a callee already read off its
/// receiver, on that receiver (§13.3.6.1 EvaluateCall, §7.3.14 Call).
pub(super) const CALL_WITH_THIS: &str = "__torajs_call_with_this";

/// The base under the optional link the chain `top` hangs from.
pub(super) fn link_base(ast: &Ast, top: ExprId) -> ExprId {
    match ast.get_expr(top) {
        Expr::Member { obj, .. } | Expr::Index { obj, .. } => link_base(ast, *obj),
        Expr::OptChain { obj, .. } | Expr::OptIndex { obj, .. } => *obj,
        other => panic!("guarded lift on a chain without an optional link: {other:?}"),
    }
}

/// `e` spelled again down to its optional link, whose base reads the
/// temp `base`, known not to be nullish there, and whose `?.` is gone.
pub(super) fn past_link(ast: &mut Ast, e: ExprId, base: &str) -> ExprId {
    let step = match ast.get_expr(e).clone() {
        Expr::Member { obj, name } => {
            let obj = past_link(ast, obj, base);
            return ast.add_expr(Expr::Member { obj, name });
        }
        Expr::Index { obj, index } => {
            let obj = past_link(ast, obj, base);
            return ast.add_expr(Expr::Index { obj, index });
        }
        Expr::OptChain { name, .. } => Step::Member(name),
        Expr::OptIndex { index, .. } => Step::Index(index),
        other => panic!("guarded lift on a chain without an optional link: {other:?}"),
    };
    let b = ast.add_expr(Expr::Ident(base.to_string()));
    let obj = ast.add_expr(Expr::As {
        expr: b,
        ty_ann: "__nonnull__".into(),
    });
    match step {
        Step::Member(name) => ast.add_expr(Expr::Member { obj, name }),
        Step::Index(index) => ast.add_expr(Expr::Index { obj, index }),
    }
}

/// The plain (non-optional) step past a bound base.
pub(super) enum Step {
    Member(String),
    Index(ExprId),
}

/// A fresh read of the temp `base`, and of `step` past it.
pub(super) fn read(ast: &mut Ast, base: &str, step: Option<Step>) -> ExprId {
    let obj = ast.add_expr(Expr::Ident(base.to_string()));
    match step {
        None => obj,
        Some(Step::Member(name)) => ast.add_expr(Expr::Member { obj, name }),
        Some(Step::Index(index)) => ast.add_expr(Expr::Index { obj, index }),
    }
}

/// `e != null`: neither null nor undefined (§13.11 IsLooselyEqual).
pub(super) fn present(ast: &mut Ast, e: ExprId) -> ExprId {
    let null = ast.add_expr(Expr::Null);
    ast.add_expr(Expr::BinOp {
        op: BinOp::LooseNeq,
        left: e,
        right: null,
    })
}
