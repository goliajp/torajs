//! `s.{at,charAt,charCodeAt,codePointAt,repeat}(Any)` 1-arg
//! String-receiver Any-widen arm extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 245 — thirty-eighth sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! S332 — per ES §22.1.3.{1,2,3,4,17} step 2-3 (or step 1 for
//! `repeat`): `ToIntegerOrInfinity` accepts arbitrary-typed
//! input. The method-table sig `(Number) -> X` rejected
//! everything but a Number, so this lane grew one admission
//! wedge per operand shape — `Any` here, `Undefined` next
//! door. Rotation 463 turned it into what the spec step
//! actually is: `pos` is COERCED, so every shape is admitted
//! and the ssa_lower mirror runs each through
//! [`crate::ssa_lower::LowerCtx::lower_to_number_operand`]
//! → `coerce_to_i64` → helper. `'abcd'.charAt('x')` is `'a'`,
//! not a type error.
//!
//! Number itself is left to fall through to the strict method
//! table (the typed-tier fast path never boxes), and
//! `Undefined` is claimed earlier by
//! [`crate::check_type_of_call_string_char_undef`], which
//! answers with the spec's `0` default rather than `NaN → 0`
//! — same answer, one fewer instruction.
//!
//! Returns `Type::String` for `at` / `charAt` / `repeat`,
//! `Type::Number` for `charCodeAt` / `codePointAt`, only
//! when the receiver is `Type::String`. `Some(Err(_))` on
//! recursive `type_of` failure. `None` otherwise (cascade
//! falls through to the strict method table).

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
    if !matches!(
        m_name.as_str(),
        "at" | "charAt" | "charCodeAt" | "codePointAt" | "repeat"
    ) || args.len() != 1
    {
        return None;
    }
    let src_ty = match checker.type_of(ast, *src_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if !matches!(src_ty, Type::String) {
        return None;
    }
    let aty = match checker.type_of(ast, args[0]) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    // Number is the strict table's own business; everything else
    // is a ToIntegerOrInfinity operand.
    if matches!(aty, Type::Number) {
        return None;
    }
    Some(Ok(
        if matches!(m_name.as_str(), "charAt" | "at" | "repeat") {
            Type::String
        } else {
            Type::Number
        },
    ))
}
