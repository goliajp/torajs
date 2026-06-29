//! `xs.{copyWithin,fill}(a, b, c, ...trailing)` Array-
//! receiver trailing-arg ignore wedge arm extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 264 — fifty-seventh sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! S246 + S310 — Array<T>.{copyWithin,fill}(a, b, c,
//! ...trailing) trailing-arg ignore per ES §23.1.3.{4,7}.
//! Spec reserves slots past the 3 useful args (target /
//! value + start + end) but tora's inline copyWithin / fill
//! loops are 3-arg only. Trailing operands type_of'd for
//! side effects then dropped at lower-time; SSA-emit reads
//! only args[0..=2] so args[3..] are silently ignored
//! without any SSA-side change. ssa_lower's S298 skip(3)
//! loop already drains args[3..] for side-effect lowering.
//!
//! Arity gate: `args.len() >= 4` — pure 1-3 arg shapes are
//! handled by the per-method 1-3-arg arms (sibling modules
//! [`crate::check_type_of_call_array_copy_within`] and
//! [`crate::check_type_of_call_array_fill`]).
//!
//! Returns:
//! - `Some(Ok(Type::Array(<elem>)))` on success
//! - `Some(Err(_))` on receiver / arg type_of failure OR on
//!   arg 0/1/2 type mismatch (exhaustive once gated to
//!   Array receiver with arity >= 4)
//! - `None` otherwise (non-Member callee, m_name not in
//!   {copyWithin, fill}, args.len() < 4, or non-Array
//!   receiver — cascade falls through to the reduce /
//!   general method dispatch arms below)

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
    if !matches!(m_name.as_str(), "copyWithin" | "fill") || args.len() < 4 {
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
    let aty0 = match checker.type_of(ast, args[0]) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if m_name == "fill" {
        if aty0 != inner && !matches!(inner, Type::Any) {
            return Some(Err(format!(
                "Array.fill arg 0 (value) must match elem type {:?}, got {aty0:?}",
                inner
            )));
        }
    } else if !matches!(aty0, Type::Number | Type::Undefined) {
        return Some(Err(format!(
            "Array.copyWithin arg 0 (target) must be number, got {aty0:?}"
        )));
    }
    let aty1 = match checker.type_of(ast, args[1]) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if !matches!(aty1, Type::Number | Type::Undefined) {
        return Some(Err(format!(
            "Array.{m_name} arg 1 must be number, got {aty1:?}"
        )));
    }
    let aty2 = match checker.type_of(ast, args[2]) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if !matches!(aty2, Type::Number | Type::Undefined) {
        return Some(Err(format!(
            "Array.{m_name} arg 2 must be number, got {aty2:?}"
        )));
    }
    for &a in args.iter().skip(3) {
        if let Err(e) = checker.type_of(ast, a) {
            return Some(Err(e));
        }
    }
    Some(Ok(Type::Array(elem.clone())))
}
