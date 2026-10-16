//! The for-in mid-loop-delete guard of [`super`], moved out at the
//! 500-line file limit.

use crate::ssa::{IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::LowerCtx;

/// chunk B3 (RFC 20260711 for-in) — mid-loop-delete guard: the loop
/// walks the key snapshot `Object.__forinKeys` took at entry, but ES
/// §14.7.5 EnumerateObjectProperties forbids visiting a property
/// deleted during enumeration. Re-check the key against the live
/// object (`any_has_property` — §7.3.11, own + user [[Prototype]]
/// chain, so an inherited key from the chain walk stays live) and
/// skip straight to the step block when it's gone. Only an `any`
/// receiver can mutate mid-loop (`delete` pins any receivers at
/// typecheck), so typed sources skip the guard entirely — their key
/// set is fixed.
pub(super) fn emit_forin_guard(
    ctx: &mut LowerCtx,
    obj_eid: crate::ast::ExprId,
    key_val: &Operand,
    step_blk: crate::ssa::BlockId,
) {
    let obj_op = ctx.lower_expr(obj_eid);
    let Some(has) = forin_key_live(ctx, obj_op, *key_val) else {
        return;
    };
    let cond = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Ne, Operand::Value(has), Operand::ConstI64(0)),
        Type::Bool,
        None,
    );
    let live_blk = ctx.f.add_block();
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(cond),
            then_blk: live_blk,
            else_blk: step_blk,
        },
    );
    ctx.cur_block = live_blk;
}

/// Is `key` still a property of the for-in object (I64 0 / 1)? `None`
/// for a receiver whose key set cannot change mid-loop. A typed Arr
/// receiver can shrink (`a.length = 1` / `splice` — RFC 20260721 刀 12
/// G16); its check rides the same §7.3.11 walk through the
/// borrow-boxing kernel.
pub(crate) fn forin_key_live(
    ctx: &mut LowerCtx,
    obj_op: Operand,
    key: Operand,
) -> Option<crate::ssa::ValueId> {
    let fid = match ctx.operand_ty(&obj_op) {
        Type::Any => ctx.intrinsics.any_has_property,
        Type::Arr(_) => ctx.intrinsics.arr_forin_key_live,
        _ => return None,
    };
    Some(ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(fid, vec![obj_op, key]),
        Type::I64,
        None,
    ))
}
