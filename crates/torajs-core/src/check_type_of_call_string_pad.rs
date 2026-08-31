//! `s.{padStart,padEnd}(maxLength?[, fillStr?])` String-
//! receiver 0-2-arg arm extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 253 — forty-sixth sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! V3-18 m1.h.45 — String.padStart / padEnd with 1 arg
//! defaults the fill string to " " per JS spec §21.1.3.16.
//! Pre-fix tora declared the methods with 2 fixed params
//! so `s.padStart(3)` failed at the arity check.
//!
//! S201 — extend the same default-undefined rule to the
//! 0-arg call per ES §22.1.3.{16,17} step 1:
//! `intMaxLength = ToLength(undefined) = 0`, so the
//! step-2 short-circuit `intMaxLength <= S.length`
//! makes the no-arg form a no-op returning S unchanged.
//!
//! S223 — widen the same default-undefined rule to a
//! 1- or 2-arg call with an explicit `undefined` in the
//! maxLength slot. Routing the standard 2-arg shape
//! (`s.padStart(3, "*")`) through the same carve-out is
//! equivalent to the line-3413 Function sig but lets us
//! accept Undefined for arg 0.
//!
//! S338 — `s.{padStart,padEnd}(Any [, fillStr])`
//! per ES §22.1.3.{16,17} step 1: ToLength accepts
//! arbitrary-typed input. Sister to S332/S334.
//! ssa_lower mirror routes Any through
//! anyv_to_number → coerce_to_i64.
//!
//! S236 — accept Undefined for the fillStr slot per ES
//! §22.1.3.{16,17} step 6.a: if fillString is undefined,
//! set it to " ". ssa_lower_str's V3-18 m1.h.45 1-arg
//! fallthrough already supplies the " " default, so we
//! just need the type gate to accept the typed-Undefined
//! operand.
//!
//! Returns `Some(Ok(Type::String))` when the receiver is
//! `Type::String` AND args.len() <= 2 AND arg 0 (if any)
//! is `Number | Undefined | Any` AND arg 1 (if any) is
//! `String | Undefined`. `Some(Err(_))` on arg type
//! mismatch on a String-receiver (exhaustive — does NOT
//! fall through once gated to String) or recursive
//! `type_of` failure. `None` otherwise (non-pad callee,
//! arity > 2, or non-String receiver — cascade falls
//! through to the join / slice / substring / general
//! method dispatch arms below).

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
    if (m_name != "padStart" && m_name != "padEnd") || args.len() > 2 {
        return None;
    }
    let src_ty = match checker.type_of(ast, *src_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if !matches!(src_ty, Type::String) {
        return None;
    }
    if let Some(arg0) = args.first() {
        // Rotation 545 — maxLength runs ToLength (§22.1.3.16.1
        // step 3): coerced, not shape-checked; the numslot lowering
        // owns the shape dispatch (rotation 463 charCodeAt
        // precedent). The fill slot below stays gated — ToString,
        // a different family.
        if let Err(e) = checker.type_of(ast, *arg0) {
            return Some(Err(e));
        }
    }
    if let Some(arg1) = args.get(1) {
        let aty = match checker.type_of(ast, *arg1) {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        if !matches!(aty, Type::String | Type::Undefined) {
            return Some(Err(format!(
                "String.{m_name} arg 1 must be string, got {aty:?}"
            )));
        }
    }
    Some(Ok(Type::String))
}
