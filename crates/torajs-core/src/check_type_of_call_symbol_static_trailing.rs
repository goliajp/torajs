//! `Symbol.{for,keyFor}(key, ...trailing)` Symbol-namespace
//! trailing-arg ignore wedge extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 286 — seventy-eighth sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! S259 — `Symbol.{for,keyFor}(key, ...trailing)` trailing-
//! arg ignore per ES §19.4.{2,3}. Spec reads only `args[0]`
//! (`Symbol.for(key)` registers / returns a symbol;
//! `Symbol.keyFor(sym)` returns the key string or
//! `undefined`); tora silent-drops `args[1..]`. SSA-emit
//! mirror widens the `args.len() == 1` gate to `>= 1`
//! (ssa_lower.rs ~18877).
//!
//! Receiver / m_name / arity matrix:
//! - `Expr::Ident("Symbol")` + m_name ∈ {"for", "keyFor"}
//!   + args.len() >= 2:
//!     - typecheck-drop all args
//!     - returns:
//!       - `Some(Ok(Type::Symbol))` for `for`
//!       - `Some(Ok(Type::Nullable(Box::new(Type::String))))`
//!         for `keyFor`
//!
//! Returns:
//! - `Some(Ok(_))` on success
//! - `Some(Err(_))` on arg type_of failure
//! - `None` otherwise (non-Member callee, non-`Symbol`
//!   namespace, m_name not in the pair, or args.len() < 2 —
//!   cascade falls through to the S248 Set.add/Map.set
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
        obj: ns_id,
        name: m_name,
    } = ast.get_expr(*callee)
    else {
        return None;
    };
    let Expr::Ident(ns) = ast.get_expr(*ns_id) else {
        return None;
    };
    if ns != "Symbol" {
        return None;
    }
    if !matches!(m_name.as_str(), "for" | "keyFor") {
        return None;
    }
    if args.len() < 2 {
        return None;
    }
    if let Err(e) = checker.type_of(ast, args[0]) {
        return Some(Err(e));
    }
    for &arg in args.iter().skip(1) {
        if let Err(e) = checker.type_of(ast, arg) {
            return Some(Err(e));
        }
    }
    Some(Ok(match m_name.as_str() {
        "for" => Type::Symbol,
        "keyFor" => Type::Nullable(Box::new(Type::String)),
        _ => unreachable!(),
    }))
}
