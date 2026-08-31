//! `xs.at(idx)` 1-arg Array-receiver arm extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 240 — thirty-third sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! S225 / rotation 544 — ES §23.1.3.1 step 2 is
//! `ToIntegerOrInfinity(index)`, which reaches every value.
//! This lane used to grow one admission wedge per operand
//! shape (`Number`, then `Undefined`) and refuse the rest by
//! name; the String siblings were turned into the coercion
//! the spec step actually is by rotation 463, and this is the
//! Array half of that. `[9, 8, 7].at('1')` is 8, `.at({})` is
//! 9, and `.at(anyIdx)` is whatever the box holds — none of
//! them a type error. ssa_lower mirrors it through
//! [`crate::ssa_lower::LowerCtx::lower_to_index_operand`],
//! whose `lower_to_number_operand` half passes a Number
//! straight through, so the typed tier still never boxes.
//! The explicit-`undefined` short-circuit to `ConstI64(0)`
//! stays there: same answer, one fewer instruction.
//!
//! Returns `Type::Array` element type on match;
//! `Some(Err(_))` when the index expression itself fails to
//! type; `None` for non-Array receiver, non-`at`, or
//! arity ≠ 1.

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
    if m_name != "at" || args.len() != 1 {
        return None;
    }
    let src_ty = match checker.type_of(ast, *src_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    let Type::Array(elem) = &src_ty else {
        return None;
    };
    // §23.1.3.1 step 2 is ToIntegerOrInfinity, which reaches every
    // value — `[9, 8, 7].at('1')` is 8 and `.at({})` is 9, neither a
    // type error. The shape gate that used to stand here admitted
    // Number and Undefined and refused the rest, so the sibling
    // `Array<Any>` element that rotation 544 stopped mistyping
    // arrived here and was refused by name. `type_of` still runs:
    // it records the operand's type and propagates its errors.
    // ssa_lower mirrors this through `lower_to_index_operand`.
    if let Err(e) = checker.type_of(ast, args[0]) {
        return Some(Err(e));
    }
    Some(Ok((**elem).clone()))
}
