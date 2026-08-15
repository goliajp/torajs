//! RFC 20260708-variadic — rest-param callee admit wedge for the
//! general call arm: a callee whose param list ends with the
//! `Type::Rest(elem)` sentinel (`(...args: E[]) => R` annotation)
//! accepts any arity at/past the fixed prefix. The wedge pops the
//! sentinel and extends the param list with one `elem` per
//! rest-position argument, so the downstream arity gate and per-arg
//! subtype loop pair every argument naturally (elem `Any` admits
//! anything through the existing Any widening).
//!
//! Runs right after the closure-argc wedge and BEFORE t28_pad — with
//! the params already stretched to the argument count, the default-
//! pad probe sees equal lengths and never mis-pads `undefined` into
//! a rest slot.

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

/// RFC 20260815-fn-value-rest-spread 刀 1 — the packed-direct-call
/// fold. `apply_rest_args` (name-keyed, pre-checker) packed every
/// DIRECT call's trailing args into one array literal, so a
/// bare-global-Ident callee's rest sentinel folds back to the one
/// `Array(elem)` param the packed site feeds. A fn VALUE (a local
/// binding, a param, a member read) has no packing site and keeps
/// the per-argument sentinel for [`apply`] to stretch.
pub(crate) fn fold_packed_direct_call(
    checker: &Checker,
    ast: &Ast,
    callee: ExprId,
    params: &mut Vec<Type>,
) {
    if !matches!(params.last(), Some(Type::Rest(_))) {
        return;
    }
    let Expr::Ident(n) = ast.get_expr(callee) else {
        return;
    };
    if checker.lookup(n).is_some() || !checker.globals.contains_key(n) {
        return;
    }
    let Some(Type::Rest(elem)) = params.pop() else {
        unreachable!("guarded by the matches! above");
    };
    params.push(Type::Array(elem));
}

pub(crate) fn apply(params: &mut Vec<Type>, effective_args: &[ExprId]) -> Result<(), String> {
    if !matches!(params.last(), Some(Type::Rest(_))) {
        return Ok(());
    }
    let Some(Type::Rest(elem)) = params.pop() else {
        unreachable!("guarded by the matches! above");
    };
    let fixed = params.len();
    if effective_args.len() < fixed {
        return Err(format!(
            "expected at least {fixed} argument(s), got {}",
            effective_args.len()
        ));
    }
    params.extend(std::iter::repeat_n(*elem, effective_args.len() - fixed));
    Ok(())
}
