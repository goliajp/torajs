//! `n.{toFixed,toExponential,toPrecision}()` 0-arg or
//! 1-arg-undefined arm extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 249 — forty-second sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! V3-18 m1.h.46 — `Number.toFixed` / `toExponential` /
//! `toPrecision` with 0 args. Per JS spec §21.1.3.3 etc:
//!   `n.toFixed()`       defaults to digits = 0
//!   `n.toExponential()` defaults to fractionDigits = "as
//!                       few as needed" (we use 6 — bun
//!                       matches; actual spec call ToInteger
//!                       on undefined gives 0 but bun's
//!                       output uses default precision)
//!   `n.toPrecision()`   no precision = same as toString
//!
//! Pre-fix tora declared with 1 fixed param so 0-arg calls
//! failed at the arity check. Implementation: typecheck-
//! only pass through; ssa_lower handles the missing-arg
//! defaults.
//!
//! S229 — accept the 1-arg explicit-Undefined shape per
//! ES §21.1.3.{3,5,6} where an undef digits/precision arg
//! folds to the same default as the 0-arg form.
//!
//! Rotation 463 — that Undefined admission was one shape out of a
//! set the spec never enumerates: §21.1.3.{3,5,6} step 1 is
//! `ToIntegerOrInfinity(fractionDigits)`, a COERCION, so
//! `(1.5).toFixed("1")` is `"1.5"` and not a type error. Every
//! non-Number shape is admitted here; `Number` alone falls through
//! to the strict method table.
//!
//! Returns `Some(Ok(Type::String))` when the receiver is
//! `Type::Number` AND args.is_empty() OR (args.len() == 1 AND arg0
//! is anything but `Type::Number`). `Some(Err(_))` on recursive
//! `type_of` failure. `None` otherwise (cascade falls through to
//! the strict method table).

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
    if !matches!(m_name.as_str(), "toFixed" | "toExponential" | "toPrecision") {
        return None;
    }
    let arity_ok = if args.is_empty() {
        true
    } else if args.len() == 1 {
        let aty = match checker.type_of(ast, args[0]) {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        !matches!(aty, Type::Number)
    } else {
        false
    };
    if !arity_ok {
        return None;
    }
    let src_ty = match checker.type_of(ast, *src_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if !matches!(src_ty, Type::Number) {
        return None;
    }
    Some(Ok(Type::String))
}
