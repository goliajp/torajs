//! The proto half of the class-synth registration family — the two
//! prologue calls that wire a class's `__proto_<C>` binding into the
//! runtime: the by-tag registry (`__torajs_proto_register`) and the
//! narrow proto-chain link (`__torajs_proto_link_fresh`). Split from
//! `ssa_lower_call_class_synth` when that dispatch file reached the
//! 500-line hard limit (its own rotation-458 watch: new arms go to a
//! sibling).

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand};
use crate::ssa_lower::LowerCtx;

/// `__torajs_proto_link_fresh(__proto_<Sub>, __proto_<Super>)` —
/// the prologue's proto-chain wire (RFC 20260825-inject-narrow-define
/// 刀 4a). Both operands are the prologue's freshly minted proto
/// dynobjs (Any-typed module bindings); the kernel writes the
/// `\x00proto` simulation entry directly, so the generic
/// member-assign route (and with it the whole member-set world)
/// carries no reloc from the injected prologue.
pub(crate) fn try_lower_proto_link_fresh(
    ctx: &mut LowerCtx<'_>,
    args: &[ExprId],
) -> Option<Operand> {
    if args.len() != 2 {
        return None;
    }
    let sub_op = ctx.lower_expr(args[0]);
    let super_op = ctx.lower_expr(args[1]);
    let cur_block = ctx.cur_block;
    let proto_link_fresh = ctx.intrinsics.proto_link_fresh;
    ctx.f.append_void(
        cur_block,
        InstKind::Call(proto_link_fresh, vec![sub_op, super_op]),
    );
    Some(Operand::ConstI64(0))
}

pub(crate) fn try_lower_proto_register(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Option<Operand> {
    if args.len() != 2 {
        return None;
    }
    let Expr::String(cname) = ctx.ast.get_expr(args[1]) else {
        return None;
    };
    let cname = cname.to_string_lossy_owned();
    let cname = cname.clone();
    let proto_op = ctx.lower_expr(args[0]);
    let cur_block = ctx.cur_block;
    if let Some(tag) = ctx.class_name_to_tag.get(&cname).copied() {
        let proto_register = ctx.intrinsics.proto_register;
        ctx.f.append_void(
            cur_block,
            InstKind::Call(
                proto_register,
                vec![Operand::ConstI64(tag as i64), proto_op],
            ),
        );
    } else {
        // Drop the lowered proto operand if no tag — keeps the path
        // well-typed if it ever fires.
        let proto_ty = ctx.operand_ty(&proto_op);
        ctx.emit_drop_value(proto_op, proto_ty);
    }
    Some(Operand::ConstI64(0))
}
