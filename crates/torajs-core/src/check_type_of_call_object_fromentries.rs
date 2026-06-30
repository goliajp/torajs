//! `Object.fromEntries(entries, ...trailing)` Object-
//! namespace 1-arg-spec trailing-arg ignore wedge arm
//! extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 277 — seventieth sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! S309 — `Object.fromEntries(entries, ...trailing)` per ES
//! §20.1.2.7: spec sig is 1-arg (entries iterable); trailing
//! args MUST be silently ignored. Pre-S309 the fixed
//! (`Type::Object("Object")`, "fromEntries") method-table sig
//! rejected `args.len() >= 2` with "expected 1 argument(s),
//! got M". typecheck-drop args[1..] + ssa_lower mirror
//! (LetDecl fast-path widens is_fromentries_call gate +
//! lower-and-drop trailing).
//!
//! Receiver / m_name / arity matrix:
//! - `Type::Object("Object")` + m_name == "fromEntries"
//!   + args.len() >= 2:
//!     - args[..] type_of'd for side-effect validation
//!     - returns `Some(Ok(Type::Any))`
//!
//! Returns:
//! - `Some(Ok(Type::Any))` on success
//! - `Some(Err(_))` on receiver / arg type_of failure
//! - `None` otherwise (non-Member callee, m_name !=
//!   "fromEntries", args.len() < 2, or non-Object receiver
//!   — cascade falls through to the S210 String.search()
//!   short-circuit wedge and beyond)

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
    if m_name != "fromEntries" {
        return None;
    }
    if args.len() < 2 {
        return None;
    }
    let src_ty = match checker.type_of(ast, *src_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if !matches!(src_ty, Type::Object("Object")) {
        return None;
    }
    for &a in args.iter() {
        if let Err(e) = checker.type_of(ast, a) {
            return Some(Err(e));
        }
    }
    Some(Ok(Type::Any))
}
