//! `Map.{keys,values,entries}(...trailing)` /
//! `Set.{keys,values,entries}(...trailing)` iterator-factory
//! trailing-arg-ignore arm extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 237 — thirtieth sub-batch of check_type_of_call.rs
//! per-shape decomposition).
//!
//! **S277** — per ES §23.{1,2}.3.X, the iterator-factory
//! methods are defined 0-arg only; trailing slots silent-drop.
//! tora's static sig is `Vec::new() → MapIter`; 1+ args bounce
//! at strict arity. Accept any arg count and typecheck-and-
//! drop trailing exprs.
//!
//! Returns `Some(Ok(MapIter))` on Map/Set-receiver match with
//! at least 1 arg (the trailing); `Some(Err(_))` on typecheck
//! error in a trailing slot; `None` otherwise (non-Map/Set
//! receiver, non-{keys,values,entries}, or 0-arg case which
//! falls to the regular method-table path).

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
    if !matches!(m_name.as_str(), "keys" | "values" | "entries") || args.is_empty() {
        return None;
    }
    let src_ty = match checker.type_of(ast, *src_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if !matches!(src_ty, Type::Map | Type::Set) {
        return None;
    }
    for &a in args.iter() {
        if let Err(e) = checker.type_of(ast, a) {
            return Some(Err(e));
        }
    }
    Some(Ok(Type::MapIter))
}
