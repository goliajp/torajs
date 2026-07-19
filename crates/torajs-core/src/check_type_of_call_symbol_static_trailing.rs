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
//!   + args.len() >= 1:
//!     - typecheck-drop all args
//!     - `args[0]` typed Any → `Some(Ok(Type::Any))` (RFC
//!       20260720-symbol-any-call-boundary: routes to the
//!       any-lane kernels, result is NaN-box bits)
//!     - otherwise, args.len() >= 2 returns:
//!       - `Some(Ok(Type::Symbol))` for `for`
//!       - `Some(Ok(Type::Nullable(Box::new(Type::String))))`
//!         for `keyFor`
//!
//! Returns:
//! - `Some(Ok(_))` on success
//! - `Some(Err(_))` on arg type_of failure
//! - `None` otherwise (non-Member callee, non-`Symbol`
//!   namespace, m_name not in the pair, or the typed 1-arg
//!   form — cascade falls through to the S248 Set.add/Map.set
//!   sibling, the general_call tail types the 1-arg form)

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
    if args.is_empty() {
        return None;
    }
    let arg0_ty = match checker.type_of(ast, args[0]) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    for &arg in args.iter().skip(1) {
        if let Err(e) = checker.type_of(ast, arg) {
            return Some(Err(e));
        }
    }
    // RFC 20260720-symbol-any-call-boundary — an Any arg routes to
    // the any-lane kernels (ToString / brand-check semantics live
    // there) and answers NaN-box bits, so the call types Any rather
    // than Symbol / Nullable<String>: a typed Nullable consumer
    // would deref VALUE_UNDEFINED as a pointer.
    if arg0_ty == Type::Any {
        return Some(Ok(Type::Any));
    }
    if args.len() < 2 {
        // Typed 1-arg form keeps its pre-existing general_call route.
        return None;
    }
    Some(Ok(match m_name.as_str() {
        "for" => Type::Symbol,
        // §20.4.2.6 answers string | undefined — a Nullable<String>
        // slot cannot spell undefined (NULL printed "null", static
        // typeof said "string"), so the call types Any and rides the
        // any-lane kernel (rotation 155, mirrors the Any-arg route).
        "keyFor" => Type::Any,
        _ => unreachable!(),
    }))
}
