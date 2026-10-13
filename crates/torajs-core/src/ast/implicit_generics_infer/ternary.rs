//! What a conditional expression gets named by the implicit
//! return-annotation sniff.
//!
//! `c ? a : b` names the annotation both branches share. A branch that
//! reads the binding the condition tests against `null` / `undefined`
//! reads it narrowed, as the checker types it
//! (`Checker::collect_null_narrow`): `(y === undefined ? 5 : y) + k`
//! with `y?: number` names `number`, not the join of `number` and
//! `__nullable(number)`, which would hand the closure an `any` return.
//! A choice between two function values names no annotation of its own
//! (`any`), as one between different types does.

use super::AstExprsView;
use crate::ast::{BinOp, Expr, ExprId};

/// The annotation of the conditional `eid`; `recur` names an operand.
pub(super) fn ternary_ann(
    exprs: AstExprsView,
    eid: ExprId,
    recur: &dyn Fn(ExprId) -> Option<String>,
) -> Option<String> {
    let Some(&Expr::Ternary {
        cond,
        then_branch: then_b,
        else_branch: else_b,
    }) = exprs.get(eid.0 as usize)
    else {
        return None;
    };
    let (mut a, mut b) = (recur(then_b)?, recur(else_b)?);
    if let Some((name, excluded_in_then)) = nullish_test(exprs, cond) {
        let (branch, ann) = if excluded_in_then {
            (then_b, &mut a)
        } else {
            (else_b, &mut b)
        };
        let reads = matches!(exprs.get(branch.0 as usize), Some(Expr::Ident(n)) if *n == name);
        if let Some(inner) = ann
            .strip_prefix("__nullable(")
            .and_then(|r| r.strip_suffix(')'))
            .filter(|_| reads)
        {
            *ann = inner.to_string();
        }
    }
    // a choice between two function values holds a closure cell on
    // either side, which the bare-function spelling `__fn(` would
    // misname; the sniff has no closure spelling for it
    let bare_fn = a
        .strip_prefix("__nullable(")
        .unwrap_or(&a)
        .starts_with("__fn(");
    if a == b && !bare_fn {
        Some(a)
    } else {
        Some("any".into())
    }
}

/// The binding `cond` compares with `null` or `undefined`, and whether
/// the then-branch is the one that rules them out (`!==` / `!=`).
fn nullish_test(exprs: AstExprsView<'_>, cond: ExprId) -> Option<(&str, bool)> {
    let Some(Expr::BinOp { op, left, right }) = exprs.get(cond.0 as usize) else {
        return None;
    };
    let excluded_in_then = match op {
        BinOp::Neq | BinOp::LooseNeq => true,
        BinOp::Eq | BinOp::LooseEq => false,
        _ => return None,
    };
    let nullish =
        |e: &Expr| matches!(e, Expr::Null) || matches!(e, Expr::Ident(n) if n == "undefined");
    let (l, r) = (exprs.get(left.0 as usize)?, exprs.get(right.0 as usize)?);
    match (l, r) {
        (Expr::Ident(n), x) | (x, Expr::Ident(n)) if n != "undefined" && nullish(x) => {
            Some((n.as_str(), excluded_in_then))
        }
        _ => None,
    }
}
