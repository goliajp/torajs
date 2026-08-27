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

/// Stretch a rest-declaring callee's param list to the call's arity,
/// answering whether the callee was variadic at all.
///
/// A call that stops SHORT of the fixed prefix is not refused here:
/// §10.2.11 binds the missing parameters to undefined and the tail to
/// the empty array, which is what
/// [`crate::check_type_of_call_t28_pad`] already spells for every
/// other callee. Leaving the param list at the fixed prefix hands it
/// that rule unchanged — and a missing slot it cannot pad (a TYPED
/// one; undefined does not fit) falls through to the arity error,
/// which words itself off the answer below because a variadic callee
/// has no exact arity to report.
pub(crate) fn apply(params: &mut Vec<Type>, effective_args: &[ExprId]) -> bool {
    if !matches!(params.last(), Some(Type::Rest(_))) {
        return false;
    }
    let Some(Type::Rest(elem)) = params.pop() else {
        unreachable!("guarded by the matches! above");
    };
    let fixed = params.len();
    params.extend(std::iter::repeat_n(
        *elem,
        effective_args.len().saturating_sub(fixed),
    ));
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(n: usize) -> Vec<ExprId> {
        (0..n as u32).map(ExprId).collect()
    }

    #[test]
    fn a_fixed_callee_is_not_variadic() {
        let mut params = vec![Type::Number, Type::Number];
        assert!(!apply(&mut params, &ids(1)));
        assert_eq!(params, vec![Type::Number, Type::Number]);
    }

    #[test]
    fn the_tail_takes_one_element_type_per_argument_past_the_prefix() {
        let mut params = vec![Type::Number, Type::Rest(Box::new(Type::Any))];
        assert!(apply(&mut params, &ids(3)));
        assert_eq!(params, vec![Type::Number, Type::Any, Type::Any]);
    }

    #[test]
    fn a_call_short_of_the_prefix_keeps_the_prefix_for_the_pad() {
        // `g()` on `(x, ...r) =>` — the shortfall is on the FIXED
        // prefix, which is the undefined-pad rule's own question.
        let mut params = vec![Type::Any, Type::Rest(Box::new(Type::Any))];
        assert!(apply(&mut params, &ids(0)));
        assert_eq!(params, vec![Type::Any], "no tail, and nothing dropped");
    }

    #[test]
    fn a_call_reaching_exactly_the_prefix_has_an_empty_tail() {
        let mut params = vec![Type::Any, Type::Rest(Box::new(Type::Number))];
        assert!(apply(&mut params, &ids(1)));
        assert_eq!(params, vec![Type::Any]);
    }
}
