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
    let allow_undef = m_name == "substring" || m_name == "slice";
    for &aid in args {
        let aty = match checker.type_of(ast, aid) {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        if aty != Type::Number && !(allow_undef && aty == Type::Undefined) && aty != Type::Any {
            return Some(Err(format!(
                "String.{m_name} arg must be number, got {aty:?}"
            )));
        }
    }
    Some(Ok(Type::String))
}
