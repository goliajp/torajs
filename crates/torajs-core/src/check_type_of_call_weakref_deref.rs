//! `WeakRef.deref(...trailing)` trailing-arg ignore wedge
//! arm extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 279 — seventy-second sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! S325 — `WeakRef.deref(...trailing)` per ES §26.1.3.2:
//! spec is 0-arg; trailing args MUST be silently ignored.
//! Pre-S325 tora's static-table sig
//! `(Type::WeakRef, "deref") -> Function([], Nullable<Any>)`
//! rejected 1+ arg calls at strict arity. Carve-out
//! typecheck-and-drops args[..]; ssa_lower mirror peeks the
//! receiver via expr_types and lower-and-drops trailing.
//!
//! Receiver / m_name / arity matrix:
//! - `Type::WeakRef` + m_name == "deref" + !args.is_empty():
//!     - args[..] type_of'd for side-effect validation
//!     - returns `Some(Ok(Type::Nullable(Box::new(Type::Any))))`
//!
//! Returns:
//! - `Some(Ok(Type::Nullable(Box::new(Type::Any))))` on success
//! - `Some(Err(_))` on receiver / arg type_of failure
//! - `None` otherwise (non-Member callee, m_name != "deref",
//!   args.is_empty(), or non-WeakRef receiver — cascade falls
//!   through to the S324 String.search trailing-ignore wedge
//!   and beyond)

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
    if m_name != "deref" {
        return None;
    }
    if args.is_empty() {
        return None;
    }
    let src_ty = match checker.type_of(ast, *src_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if !matches!(src_ty, Type::WeakRef) {
        return None;
    }
    for &a in args.iter() {
        if let Err(e) = checker.type_of(ast, a) {
            return Some(Err(e));
        }
    }
    Some(Ok(Type::Nullable(Box::new(Type::Any))))
}
