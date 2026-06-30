//! `n.toLocaleString(locales?, options?, ...trailing)`
//! Number-receiver 3+ arg trailing-arg wedge extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 287 — seventy-ninth sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! S260 — `n.toLocaleString(locales?, options?, ...trailing)`
//! trailing-arg ignore per ES §21.1.3.4. Spec reads
//! `locales` + `options` + ignores the rest; tora ignores
//! all formatting args already (en-US-only subset;
//! ssa_lower `pass_args=false` at line ~16707). The
//! fixed-arity `vec![Any, Any] -> String` sig above rejects
//! 3+ arg calls — widen here. Type-erased silent-drop
//! matches the SSA-emit reality (no arg flows to runtime
//! helper).
//!
//! Receiver / m_name / arity matrix:
//! - `Type::Number` + m_name == "toLocaleString" +
//!   args.len() >= 3:
//!     - typecheck-drop all args
//!     - returns `Some(Ok(Type::String))`
//!
//! Returns:
//! - `Some(Ok(Type::String))` on success
//! - `Some(Err(_))` on receiver / arg type_of failure
//! - `None` otherwise (non-Member callee, m_name !=
//!   "toLocaleString", args.len() < 3, or non-Number
//!   receiver — cascade falls through to the S259 Symbol
//!   sibling and beyond)

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

pub(crate) fn try_match(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    args: &Vec<ExprId>,
) -> Option<Result<Type, String>> {
    let Expr::Member {
        obj: recv_id,
        name: m_name,
    } = ast.get_expr(*callee)
    else {
        return None;
    };
    if m_name != "toLocaleString" {
        return None;
    }
    if args.len() < 3 {
        return None;
    }
    let recv_ty = match checker.type_of(ast, *recv_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if !matches!(recv_ty, Type::Number) {
        return None;
    }
    for &arg in args.iter() {
        if let Err(e) = checker.type_of(ast, arg) {
            return Some(Err(e));
        }
    }
    Some(Ok(Type::String))
}
