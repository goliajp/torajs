//! `recv.m?.(args)` where `m` is an optional method of an interface
//! that dispatches through witness slots (§13.3.9 OptionalChain over a
//! call): the receiver evaluates once, the has face says whether the
//! member is there, and only then do the arguments evaluate and the
//! call face run — with `recv` as its receiver, as the member call it
//! is. A miss answers `undefined`. The result is an `any`, like every
//! other optional call's.

use crate::ast::{Expr, ExprId};
use crate::iface_witness::has_face;
use crate::ssa::{InstKind, Operand, Terminator, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let Expr::Member { obj, name } = ctx.ast.get_expr(callee) else {
        return None;
    };
    let (obj, name) = (*obj, name.clone());
    let iface = crate::ssa_lower_iface::receiver_iface(ctx, obj)?;
    let base = ctx.ast.method_index.len();
    ctx.ast
        .iface_slots
        .slot_of(base, &iface, &has_face(&name))?;
    let recv = ctx.lower_expr(obj);
    let has = crate::ssa_lower_iface::emit_face_call(
        ctx,
        None,
        &iface,
        &has_face(&name),
        vec![recv],
        &[],
    )?;
    let res_slot = ctx.alloca_in_entry(Type::Any, Some("__optcall"));
    let hit_blk = ctx.f.add_block();
    let miss_blk = ctx.f.add_block();
    let after = ctx.f.add_block();
    let cb = ctx.cur_block;
    ctx.f.set_term(
        cb,
        Terminator::CondBr {
            cond: has,
            then_blk: hit_blk,
            else_blk: miss_blk,
        },
    );
    ctx.cur_block = miss_blk;
    let undef = undef_box(ctx);
    ctx.f.append_void(
        miss_blk,
        InstKind::Store(undef, Operand::Value(res_slot), 0),
    );
    ctx.f.set_term(miss_blk, Terminator::Br(after));
    ctx.cur_block = hit_blk;
    ctx.proofs.enter_cond();
    let mut argv = vec![recv];
    argv.extend(args.iter().map(|a| ctx.lower_expr(*a)));
    let owned: Vec<(ExprId, Operand)> = args
        .iter()
        .zip(argv[1..].iter())
        .filter(|(a, _)| ctx.expr_owned_shape(**a))
        .map(|(a, op)| (*a, *op))
        .collect();
    let r = crate::ssa_lower_iface::emit_face_call(ctx, Some(eid), &iface, &name, argv, args)?;
    for (a, op) in owned {
        ctx.release_owned_temp(a, &op);
    }
    // the call's result is owned; boxing hands its stake to the box
    let boxed = match ctx.operand_ty(&r) {
        Type::Void => undef_box(ctx),
        Type::Any => r,
        _ => ctx.box_to_any(r),
    };
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(boxed, Operand::Value(res_slot), 0),
    );
    ctx.proofs.leave_cond();
    let hb = ctx.cur_block;
    ctx.f.set_term(hb, Terminator::Br(after));
    ctx.cur_block = after;
    let v = ctx.f.append_inst(
        after,
        InstKind::Load(Type::Any, Operand::Value(res_slot), 0),
        Type::Any,
        None,
    );
    Some(Operand::Value(v))
}

fn undef_box(ctx: &mut LowerCtx<'_>) -> Operand {
    let b = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::BoxAny(5, Operand::ConstI64(0)),
        Type::Any,
        None,
    );
    Operand::Value(b)
}
