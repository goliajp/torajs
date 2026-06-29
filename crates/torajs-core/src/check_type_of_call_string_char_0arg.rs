//! `s.{charAt,charCodeAt,codePointAt}()` 0-arg String-receiver
//! arm extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 243 — thirty-sixth sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! V3-18 wedge — `String.charAt` / `charCodeAt` / `codePointAt`
//! accept an optional pos arg per JS spec §22.1.3.4 /
//! §22.1.3.5 / §22.1.3.6: missing pos defaults to 0. Pre-fix
//! tora declared with one required param so 0-arg calls
//! bounced at the unified arity check with
//! `expected 1 argument(s), got 0`. Implementation:
//! typecheck-only pass through for the missing-arg shape;
//! ssa_lower's 1-arg path gets a synthetic `ConstI64(0)`
//! padded in for the default.
//!
//! Returns `Type::String` for `charAt`, `Type::Number` for
//! `charCodeAt` / `codePointAt` on a `Type::String` receiver.
//! `Some(Err(_))` only if recursive `type_of` on the receiver
//! fails. `None` for non-String receiver, non-charAt-family
//! method, or arity ≠ 0.

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
    if !matches!(m_name.as_str(), "charAt" | "charCodeAt" | "codePointAt") || !args.is_empty() {
        return None;
    }
    let src_ty = match checker.type_of(ast, *src_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if !matches!(src_ty, Type::String) {
        return None;
    }
    Some(Ok(if m_name == "charAt" {
        Type::String
    } else {
        Type::Number
    }))
}
