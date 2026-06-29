//! `xs.{pop,shift}(...trailing)` Array-receiver trailing-
//! arg ignore wedge arm extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 261 — fifty-fourth sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! S288 — Array<T>.{pop,shift}(...trailing) trailing-arg
//! ignore per ES §23.1.3.{20,24}. Spec sigs are 0-arg;
//! tora's runtime helpers (in-place len-- + tail/head slot
//! load) ignore any extras. Typecheck-and-drop here;
//! ssa_lower's try_arr_pop / try_arr_shift arms widen the
//! `args.is_empty()` gate + lower-and-drop args[..] so
//! step()-style exprs fire (S272 idiom).
//!
//! Returns `Some(Ok(<elem-type>))` when the receiver is
//! `Type::Array(elem)` AND m_name ∈ {pop, shift} AND
//! args.len() >= 1 (args[..] type_of'd for side effects
//! then dropped). `Some(Err(_))` on recursive `type_of`
//! failure (receiver or any arg). `None` otherwise
//! (non-Member callee, m_name mismatch, empty args, or
//! non-Array receiver — cascade falls through to the
//! reverse-join-toString / general method dispatch arms
//! below).

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
    if !matches!(m_name.as_str(), "pop" | "shift") || args.is_empty() {
        return None;
    }
    let src_ty = match checker.type_of(ast, *src_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    let Type::Array(elem) = &src_ty else {
        return None;
    };
    let inner = (**elem).clone();
    for &a in args.iter() {
        if let Err(e) = checker.type_of(ast, a) {
            return Some(Err(e));
        }
    }
    Some(Ok(inner))
}
