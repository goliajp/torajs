//! An unmarked `.value` read: the P10.4 identity (`super`).

use crate::ast::ExprId;
use crate::check::{self as check_mod};
use crate::ssa::Operand;
use crate::ssa_lower::LowerCtx;

pub(super) fn try_p10_4_primitive_identity(ctx: &mut LowerCtx<'_>, obj: ExprId) -> Option<Operand> {
    if !matches!(
        ctx.expr_types.get(&obj),
        Some(check_mod::Type::Number)
            | Some(check_mod::Type::String)
            | Some(check_mod::Type::Boolean)
            | Some(check_mod::Type::Array(_))
            | Some(check_mod::Type::BigInt)
    ) {
        return None;
    }
    Some(ctx.lower_expr(obj))
}
