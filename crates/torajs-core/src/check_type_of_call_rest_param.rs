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

use crate::ast::ExprId;
use crate::check::Type;

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
