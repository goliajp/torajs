//! `s.{slice,substring,substr}(start, end)` String-receiver
//! 2-arg wedge arm extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 256 — forty-ninth sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! S221 — `s.substring(start, end)` accepts Undefined for
//! either positional per ES §22.1.3.22 step 4/5:
//! ToIntegerOrInfinity(undef)=0 for start, end===undefined
//! takes the omitted default (len). ssa_lower mirror short-
//! circuits each undef slot to 0 / recv.length so the
//! str_substring helper's I64 ABI never sees an Undefined
//! operand.
//!
//! S232 — extend the same widen to `s.slice(start, end)`
//! 2-arg per ES §22.1.3.20 step 3/4: start undef → 0,
//! end undef → length. slice's negative-index handling is
//! orthogonal — the helper still receives concrete I64s
//! after the ssa_lower mirror substitutes.
//!
//! S333 — 2-arg form widens Any. Same Any coerce pattern A
//! as the 1-arg path. `substr` extends the method-table
//! sig (Number, Number) here so the (Any, Any) shape
//! doesn't fall through to the strict gate; ssa_lower
//! mirror decodes via anyv_to_number → coerce_to_i64.
//!
//! Returns `Some(Ok(Type::String))` when the receiver is
//! `Type::String` AND m_name is slice / substring /
//! substr AND args.len() == 2 AND every arg is
//! `Number | (Undefined iff slice|substring) | Any`.
//! `Some(Err(_))` on arg type mismatch (exhaustive once
//! gated to String) or recursive `type_of` failure. `None`
//! otherwise (non-Member callee, m_name mismatch, arity
//! != 2, or non-String receiver — cascade falls through
//! to the …trailing / general method dispatch arms).

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
    if (m_name != "substring" && m_name != "slice" && m_name != "substr") || args.len() != 2 {
        return None;
    }
    let src_ty = match checker.type_of(ast, *src_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if !matches!(src_ty, Type::String) {
        return None;
    }
    // Rotation 545 — both slots run ToIntegerOrInfinity (§22.1.3.23
    // step 3 / §22.1.3.24 step 5 / §B.2.2.1 steps 4-5): the operand
    // is COERCED, not shape-checked (`'abcdef'.slice(0, '2')` is
    // "ab"), so the checker only typechecks; the numslot lowering
    // owns the shape dispatch. Same widening as rotation 463's
    // charCodeAt precedent.
    for &aid in args {
        if let Err(e) = checker.type_of(ast, aid) {
            return Some(Err(e));
        }
    }
    Some(Ok(Type::String))
}
