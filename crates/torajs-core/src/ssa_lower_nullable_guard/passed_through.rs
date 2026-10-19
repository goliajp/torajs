//! The operands an operator answers as its own value.
//!
//! A nullish value an operand holds is the operator's value too, so a
//! source predicate asks the operands. The throw analysis
//! (`ast_throw_info::nullish`) asks the same ones, so a guard the
//! lowering arms here is a raise the callers check for.

use crate::ast::{Ast, BinOp, Expr, ExprId};

/// The operands whose value `eid` may answer as it is: both arms of a
/// conditional, the right operand of a comma, the value an assignment
/// stores, and those of a logical operator — `&&` answers a nullish
/// left operand itself, while `||` and `??` answer their left operand
/// only when it is not nullish.
#[inline]
pub(crate) fn passed_through(ast: &Ast, eid: ExprId) -> [Option<ExprId>; 2] {
    match ast.get_expr(eid) {
        Expr::Ternary {
            then_branch,
            else_branch,
            ..
        } => [Some(*then_branch), Some(*else_branch)],
        Expr::BinOp {
            op: BinOp::LAnd,
            left,
            right,
        } => [Some(*left), Some(*right)],
        Expr::BinOp {
            op: BinOp::LOr,
            right,
            ..
        }
        | Expr::Sequence { right, .. }
        | Expr::Nullish { rhs: right, .. }
        | Expr::Assign { value: right, .. } => [Some(*right), None],
        _ => [None, None],
    }
}
