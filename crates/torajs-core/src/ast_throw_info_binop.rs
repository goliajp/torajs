//! Which BINARY OPERATOR shapes can raise — lifted out of
//! [`crate::ast_throw_info`] when that file stood exactly at its size
//! limit.
//!
//! The seam is the one its own arms already draw: the parent walks the
//! expression tree and asks, at each node, *can this raise*; this file
//! answers that one question for the operators. Both predicates below
//! mirror a dispatch condition in `ssa_lower` EXACTLY — each comment
//! names which — because a mismatch either costs a throw-check on a
//! hot typed path or swallows a real throw at a function boundary.

use crate::ast::BinOp;

/// The operators `ssa_lower_binop_inner_any_arith::try_lower` routes
/// to the any-lane kernels when an operand is not a primitive.
fn is_operator_kernel_op(op: BinOp) -> bool {
    matches!(
        op,
        BinOp::Add
            | BinOp::Sub
            | BinOp::Mul
            | BinOp::Div
            | BinOp::Mod
            | BinOp::Pow
            | BinOp::BitAnd
            | BinOp::BitOr
            | BinOp::BitXor
            | BinOp::Shl
            | BinOp::Shr
            | BinOp::UShr
            | BinOp::Lt
            | BinOp::Le
            | BinOp::Gt
            | BinOp::Ge
    )
}

/// An operand type whose operator lowering never reaches a kernel
/// that records a throw: the typed number / string / bigint / boolean
/// lanes and the null-ish coercions. Everything else — `any`, any
/// object shape, a blank — may run user code or refuse a pair.
fn is_arith_safe_primitive(t: Option<&crate::check::Type>) -> bool {
    use crate::check::Type as T;
    matches!(
        t,
        Some(T::Number | T::String | T::Boolean | T::BigInt | T::Null | T::Undefined | T::Void)
    )
}

/// Whether a binary operator with these operand types can record a
/// throw. Two independent reasons, and the arm needs either.
pub(crate) fn binop_raises(
    op: BinOp,
    left: Option<&crate::check::Type>,
    right: Option<&crate::check::Type>,
) -> bool {
    // Rotation 623 — an operand the checker does not type as a
    // primitive (an `any`, an object whose ToPrimitive runs user code,
    // a blank) sends the operator to the any-lane kernels (any_add /
    // any_arith / any_bitwise / any_compare), and those record a
    // TypeError for a mixed BigInt pair, a Symbol operand or a
    // throwing `valueOf`. The lowering checks right after the call,
    // but a named fn whose only raise was this one was M4.3.b-skipped
    // at its caller: `function f(a: any) { return a * 2n }` — `f(3)`
    // printed the propagate sentinel (`[unknown-any-tag]`) and the
    // TypeError surfaced statements later, uncaught. Typed number
    // arithmetic stays unmarked (no hot-path cost).
    if is_operator_kernel_op(op)
        && (!is_arith_safe_primitive(left) || !is_arith_safe_primitive(right))
    {
        return true;
    }
    // P7.4-a-b — a bigint Div/Mod/Pow/Shl/Shr lowers to a runtime
    // helper that calls __torajs_throw_range_error (divide-by-zero /
    // negative exponent / shift too large). This mirrors ssa_lower's
    // runtime dispatch condition EXACTLY (op in the throwing set AND
    // both operands typed BigInt) so it is precise: number `/ % ** <<
    // >>` is never flagged, and every bigint throwing op is.
    matches!(
        op,
        BinOp::Div | BinOp::Mod | BinOp::Pow | BinOp::Shl | BinOp::Shr
    ) && matches!(left, Some(crate::check::Type::BigInt))
        && matches!(right, Some(crate::check::Type::BigInt))
}
