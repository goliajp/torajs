//! Arms the join settles one at a time: widened to the other arm's
//! repr, boxed into an `any` slot, or turned into a fn object against
//! a closure-cell arm.

use crate::ast::ExprId;
use crate::ssa::{Operand, Type};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_call_fn_cell as fn_cell;

/// Two arms the checker typed alike that lowered to different reprs:
/// a number at `i64` beside one at `f64` joins at `f64` (W3 S8), and
/// one `Str` arm beside a `Substr` view joins at `Str`, the view
/// copied out on its own path (`ssa_lower_logical::join_arm` module doc). None
/// for any other pair.
pub(super) fn settle_pair(
    ctx: &mut LowerCtx<'_>,
    (then_val, else_val): (Operand, Operand),
    (then_end, else_end): (&mut crate::ssa::BlockId, &mut crate::ssa::BlockId),
    (then_branch, else_branch): (ExprId, ExprId),
) -> Option<(Operand, Operand, Type, bool, bool)> {
    match (ctx.operand_ty(&then_val), ctx.operand_ty(&else_val)) {
        (Type::F64, Type::I64) => {
            ctx.cur_block = *else_end;
            let e = ctx.coerce_to_f64(else_val);
            *else_end = ctx.cur_block;
            Some((then_val, e, Type::F64, false, false))
        }
        (Type::I64, Type::F64) => {
            ctx.cur_block = *then_end;
            let t = ctx.coerce_to_f64(then_val);
            *then_end = ctx.cur_block;
            Some((t, else_val, Type::F64, false, false))
        }
        (Type::Str, Type::Substr) | (Type::Substr, Type::Str) => {
            let (t, t_minted) = ctx.settle_join_arm(then_end, then_branch, then_val, Type::Str);
            let (e, e_minted) = ctx.settle_join_arm(else_end, else_branch, else_val, Type::Str);
            Some((t, e, Type::Str, t_minted, e_minted))
        }
        _ => None,
    }
}

/// A top-level function against a cell of its signature (`c ? h :
/// o.g`, the destructuring default `{ g = h }`): the pointer arm
/// becomes its own cell, fresh, so the join is a cell whichever arm
/// ran instead of the cell being called as a code address. None when
/// the arms are not that pair, or the pointer arm is not a static
/// choice of top-level functions.
pub(super) fn fn_cell_join(
    ctx: &mut LowerCtx<'_>,
    (then_val, else_val): (Operand, Operand),
    (then_end, else_end): (&mut crate::ssa::BlockId, &mut crate::ssa::BlockId),
    (then_branch, else_branch): (ExprId, ExprId),
) -> Option<(Operand, Operand, Type, bool, bool)> {
    match (ctx.operand_ty(&then_val), ctx.operand_ty(&else_val)) {
        (Type::FnSig(a), cell @ Type::Closure(b)) if a == b => {
            ctx.cur_block = *then_end;
            let c = fn_cell::static_fn_value_cell(ctx, cell, then_branch, then_val)?;
            *then_end = ctx.cur_block;
            Some((c, else_val, cell, true, false))
        }
        (cell @ Type::Closure(a), Type::FnSig(b)) if a == b => {
            ctx.cur_block = *else_end;
            let c = fn_cell::static_fn_value_cell(ctx, cell, else_branch, else_val)?;
            *else_end = ctx.cur_block;
            Some((then_val, c, cell, false, true))
        }
        _ => None,
    }
}

/// Box one arm at the end of its own block. The box is fresh, so a
/// borrow-shape arm takes +1 first (chunk 722, see `widen_branches`),
/// and a named function boxes as its fn object.
pub(super) fn box_arm(
    ctx: &mut LowerCtx<'_>,
    end: &mut crate::ssa::BlockId,
    branch: ExprId,
    val: Operand,
) -> Operand {
    ctx.cur_block = *end;
    let is_borrow = !ctx.expr_transfers_ownership(branch);
    let boxed = ctx.box_owned_from_expr(branch, val, is_borrow);
    *end = ctx.cur_block;
    boxed
}
