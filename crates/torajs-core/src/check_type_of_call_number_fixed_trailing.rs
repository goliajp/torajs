//! `n.{toFixed,toExponential,toPrecision}(digits,
//! ...trailing)` trailing-arg ignore arm extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 250 — forty-third sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! S254 — per ES §21.1.3.{3,5,6} trailing-arg ignore. Spec
//! reads only args[0] (digits/fractionDigits/precision);
//! tora silent-drops trailing per generic trailing-arg-
//! ignore policy. SSA-emit's per-method dispatch pushes
//! args into argv; the ssa-lower S254 mirror caps the push
//! at args[0].
//!
//! Returns `Some(Ok(Type::String))` when the receiver is
//! `Type::Number` AND args.len() >= 2 AND arg0 is
//! `Type::Number | Type::Undefined`; trailing args
//! type_of'd for side effects then dropped.
//! `Some(Err(_))` on arg0 type mismatch (exhaustive —
//! does NOT fall through) or recursive `type_of` failure.
//! `None` otherwise (non-fixed-family callee, args < 2, or
//! non-Number receiver — cascade falls through).

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

pub(crate) fn try_match(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    args: &Vec<ExprId>,
) -> Option<Result<Type, String>> {
    let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
    else {
        return None;
    };
    if !matches!(m_name.as_str(), "toFixed" | "toExponential" | "toPrecision") || args.len() < 2 {
        return None;
    }
    let src_ty = match checker.type_of(ast, *src_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if !matches!(src_ty, Type::Number) {
        return None;
    }
    let arg0_ty = match checker.type_of(ast, args[0]) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if !matches!(arg0_ty, Type::Number | Type::Undefined) {
        return Some(Err(format!(
            "Number.{m_name} arg 0 must be number, got {arg0_ty:?}"
        )));
    }
    for &arg in args.iter().skip(1) {
        if let Err(e) = checker.type_of(ast, arg) {
            return Some(Err(e));
        }
    }
    Some(Ok(Type::String))
}
