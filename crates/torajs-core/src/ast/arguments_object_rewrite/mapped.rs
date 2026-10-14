//! The touches of an argv body on [`ArgcMode::MappedArgc`]: §10.4.4
//! CreateMappedArgumentsObject maps parameter `k` to `arguments[k]`
//! only when the caller passed position `k`, and on an argv body that
//! count is the hidden `__torajs_argc`. The parameter is the storage
//! of a mapped position; the materialized array holds the rest.
//!
//! [`ArgcMode::MappedArgc`]: super::super::arguments_object::ArgcMode::MappedArgc

use super::super::arguments_object::ArgcMode;
use super::super::{Ast, BinOp, Expr, ExprId};
use super::{SloppyCallee, rewrite_arguments_in_expr};

/// The position an in-range literal `arguments[k]` names, if `eid` is
/// one (the admission scan lets no other index through).
pub(super) fn mapped_position(ast: &Ast, eid: ExprId, params: &[String]) -> Option<usize> {
    let Expr::Index { obj, index } = ast.get_expr(eid) else {
        return None;
    };
    if !matches!(ast.get_expr(*obj), Expr::Ident(n) if n == "arguments") {
        return None;
    }
    match ast.get_expr(*index) {
        Expr::Number(n) if n.fract() == 0.0 && *n >= 0.0 && (*n as usize) < params.len() => {
            Some(*n as usize)
        }
        _ => None,
    }
}

/// A write whose target is a mapped `arguments[k]`, rewritten; `None`
/// for every other expression.
pub(super) fn rewrite_mapped_write(
    ast: &mut Ast,
    e: &Expr,
    params: &[String],
    from_array: bool,
    sloppy_callee: SloppyCallee<'_>,
) -> Option<ExprId> {
    match *e {
        Expr::Assign { target, value } => {
            let k = mapped_position(ast, target, params)?;
            let v = rewrite_arguments_in_expr(
                ast,
                value,
                params,
                ArgcMode::MappedArgc,
                from_array,
                sloppy_callee,
            );
            Some(mapped_assign(ast, params, k, v))
        }
        Expr::PostIncr {
            target,
            is_inc,
            is_prefix,
        } => {
            let k = mapped_position(ast, target, params)?;
            Some(mapped_update(ast, params, k, is_inc, is_prefix))
        }
        _ => None,
    }
}

/// `__torajs_argc > k`: whether position `k` is mapped in this call.
fn passed(ast: &mut Ast, k: usize) -> ExprId {
    let argc = ast.add_expr(Expr::Ident("__torajs_argc".into()));
    let pos = ast.add_expr(Expr::Number(k as f64));
    ast.add_expr(Expr::BinOp {
        op: BinOp::Gt,
        left: argc,
        right: pos,
    })
}

fn param(ast: &mut Ast, params: &[String], k: usize) -> ExprId {
    ast.add_expr(Expr::Ident(params[k].clone()))
}

fn slot(ast: &mut Ast, k: usize) -> ExprId {
    let arr = ast.add_expr(Expr::Ident("__torajs_arguments".into()));
    let index = ast.add_expr(Expr::Number(k as f64));
    ast.add_expr(Expr::Index { obj: arr, index })
}

/// A read of `arguments[k]`.
pub(super) fn mapped_read(ast: &mut Ast, params: &[String], k: usize) -> ExprId {
    let cond = passed(ast, k);
    let then_branch = param(ast, params, k);
    let else_branch = slot(ast, k);
    ast.add_expr(Expr::Ternary {
        cond,
        then_branch,
        else_branch,
    })
}

/// `arguments[k] = value` (`value` already rewritten): the store lands
/// in the array slot, and a mapped position copies it into the
/// parameter. The value expression is evaluated once.
fn mapped_assign(ast: &mut Ast, params: &[String], k: usize, value: ExprId) -> ExprId {
    let target = slot(ast, k);
    let store = ast.add_expr(Expr::Assign { target, value });
    let cond = passed(ast, k);
    let p = param(ast, params, k);
    let stored = slot(ast, k);
    let then_branch = ast.add_expr(Expr::Assign {
        target: p,
        value: stored,
    });
    let else_branch = slot(ast, k);
    let pick = ast.add_expr(Expr::Ternary {
        cond,
        then_branch,
        else_branch,
    });
    ast.add_expr(Expr::Sequence {
        left: store,
        right: pick,
    })
}

/// `arguments[k]++` and the other update forms.
fn mapped_update(
    ast: &mut Ast,
    params: &[String],
    k: usize,
    is_inc: bool,
    is_prefix: bool,
) -> ExprId {
    let cond = passed(ast, k);
    let p = param(ast, params, k);
    let then_branch = ast.add_expr(Expr::PostIncr {
        target: p,
        is_inc,
        is_prefix,
    });
    let s = slot(ast, k);
    let else_branch = ast.add_expr(Expr::PostIncr {
        target: s,
        is_inc,
        is_prefix,
    });
    ast.add_expr(Expr::Ternary {
        cond,
        then_branch,
        else_branch,
    })
}
