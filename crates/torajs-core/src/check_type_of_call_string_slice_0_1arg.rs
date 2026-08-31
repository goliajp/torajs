//! `s.{slice,substring,substr}(start?)` String-receiver
//! 0-1-arg wedge arm extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 255 — forty-eighth sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! V3-18 m1.h.36 — String.slice / substring with 0 or 1
//! args. Per JS spec §21.1.3.21 / §21.1.3.23:
//!   s.slice()      = s.slice(0, s.length)
//!   s.slice(start) = s.slice(start, s.length)
//!   (same for substring; substring also clamps and swaps
//!   args, but the optional-arity shape is identical at
//!   the call site)
//!
//! S232 — accept Undefined for the start slot per ES
//! §22.1.3.20 step 3 (slice) / §22.1.3.22 step 4
//! (substring): ToIntegerOrInfinity(undef)=0. The
//! ssa_lower_str mirror substitutes ConstI64(0) and the
//! existing 0-1-arg fallthrough fills in the end=length
//! default. `substr` is excluded from undef widening — its
//! 2nd arg is a length not an end index, and the T-49
//! carve-out below handles its own arity-undef. substr
//! still participates in this arm for the strict-Number /
//! Any cases via the 0-1-arg arity check.
//!
//! S333 — `s.{slice,substring,substr}(Any)` per ES
//! §22.1.3.{20,22,23}: ToIntegerOrInfinity accepts
//! arbitrary-typed input. Widen the strict-Number gate;
//! ssa_lower mirror routes Any through
//! anyv_to_number → coerce_to_i64 → helper. Sister to
//! S332 (charCodeAt/charAt/... Any).
//!
//! Returns `Some(Ok(Type::String))` when the receiver is
//! `Type::String` AND m_name is slice / substring /
//! substr AND args.len() < 2 AND every arg is
//! `Number | (Undefined iff slice/substring) | Any`.
//! `Some(Err(_))` on arg type mismatch (exhaustive once
//! gated to String) or recursive `type_of` failure.
//! `None` otherwise (non-Member callee, m_name mismatch,
//! arity >= 2, or non-String receiver — cascade falls
//! through to the 2-arg / general method dispatch arms).

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
    if (m_name != "slice" && m_name != "substring" && m_name != "substr") || args.len() >= 2 {
        return None;
    }
    let src_ty = match checker.type_of(ast, *src_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if !matches!(src_ty, Type::String) {
        return None;
    }
    // Rotation 545 — ToIntegerOrInfinity COERCES the slot
    // (§22.1.3.23/.24, §B.2.2.1), so the checker only typechecks;
    // the numslot lowering owns the shape dispatch. Same widening
    // as rotation 463's charCodeAt precedent.
    for &aid in args {
        if let Err(e) = checker.type_of(ast, aid) {
            return Some(Err(e));
        }
    }
    Some(Ok(Type::String))
}
