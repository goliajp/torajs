//! `String.{startsWith,endsWith,includes}(needle, position?)`
//! 2-arg arm extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 232 — twenty-fifth sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! V3-18 m1.h.51 — JS spec §21.1.3.20 / §21.1.3.6 /
//! §21.1.3.7. Pre-fix declared with 1 fixed param so 2-arg
//! calls hit the arity check; widen here.
//!
//! - **S224** — accept `Undefined` for the position slot per
//!   ES §22.1.3.{21,5} (startsWith/includes:
//!   `ToIntegerOrInfinity(undefined) = 0`) and §22.1.3.7
//!   (endsWith: `endPosition undef → length`). ssa_lower
//!   mirror lowers `undef` → `ConstI64(0)` for startsWith/
//!   includes and `ConstI64(i64::MAX)` for endsWith; the
//!   `_from` helpers clamp `> len` to `len`.
//!
//! Returns `Some(Ok(Boolean))` on String-receiver match;
//! `Some(Err(_))` on bad needle / position type; `None` for
//! non-String receivers.

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
    if !matches!(m_name.as_str(), "startsWith" | "endsWith" | "includes") || args.len() != 2 {
        return None;
    }
    let src_ty = match checker.type_of(ast, *src_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if !matches!(src_ty, Type::String) {
        return None;
    }
    let needle_ty = match checker.type_of(ast, args[0]) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if !matches!(needle_ty, Type::String) {
        return Some(Err(format!(
            "String.{m_name} arg 0 must be string, got {needle_ty:?}"
        )));
    }
    if let Err(e) = checker.type_of(ast, args[1]) {
        return Some(Err(e));
    }
    // Rotation 544 — §22.1.3.{22,7,14} coerce this slot. `endsWith`
    // is the one that reads `undefined` and NaN differently (its
    // step 5 tests the slot itself), which ssa_lower carries; the
    // typecheck owes only the admission.
    Some(Ok(Type::Boolean))
}
