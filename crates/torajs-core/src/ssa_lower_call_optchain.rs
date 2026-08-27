//! The guard half of `a?.m(args)` — §13.3.9's short circuit.
//!
//! `desugar_optchain_calls` already said the other half: the callee is
//! an ordinary `Member` read by the time lowering sees it, so every
//! dispatch lane answers the hit branch as written. What it recorded in
//! `Ast::optchain_calls` is what this lane puts back — the base is
//! evaluated ONCE, and when it is nullish the whole chain answers
//! `undefined` without the arguments ever being evaluated.
//!
//! The arguments living inside the hit block is the part that is easy
//! to lose: §13.3.9 short-circuits the chain, not just the call, so
//! `o?.g(sideEffect())` on a nullish `o` must not run `sideEffect()`.
//! Re-entering the cascade from inside that block is what buys it.
//!
//! The base is lowered here and PARKED, so the member callee's own
//! receiver read consumes it rather than evaluating it a second time —
//! the same protocol the declining dispatchers use. It is parked under
//! the base's own ExprId because the callee reads it through the
//! non-null assertion the desugar wrapped it in, and that assertion
//! lowers by lowering its inner.

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Terminator, Type};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_optindex::emit_nullish_cond;

/// Claim a call the desugar marked as an optional chain. `None` for
/// every ordinary call, so the cascade continues.
pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let base = *ctx.ast.optchain_calls.get(&eid)?;

    let recv = ctx.lower_expr(base);
    let recv_ty = ctx.operand_ty(&recv);
    let res_slot = ctx.alloca_in_entry(Type::Any, Some("__optchain_call"));
    let nullish = emit_nullish_cond(ctx, &recv, &recv_ty);

    let null_blk = ctx.f.add_block();
    let hit_blk = ctx.f.add_block();
    let after = ctx.f.add_block();
    let cur = ctx.cur_block;
    ctx.f.set_term(
        cur,
        Terminator::CondBr {
            cond: nullish,
            then_blk: null_blk,
            else_blk: hit_blk,
        },
    );

    // Short circuit — the chain's value, and nothing else runs.
    ctx.cur_block = null_blk;
    let undef = ctx.f.append_inst(
        null_blk,
        InstKind::Call(
            ctx.intrinsics.any_box,
            vec![Operand::ConstI64(5), Operand::ConstI64(0)],
        ),
        Type::Any,
        None,
    );
    ctx.f.append_void(
        null_blk,
        InstKind::Store(Operand::Value(undef), Operand::Value(res_slot), 0),
    );
    ctx.f.set_term(null_blk, Terminator::Br(after));

    // Hit — an ordinary member call, arguments and all.
    ctx.cur_block = hit_blk;
    ctx.redispatch_lowered = Some((base, recv.clone()));
    let hit = crate::ssa_lower_call::dispatch(ctx, eid, callee, args);
    let hit_ty = ctx.operand_ty(&hit);
    let hit_any = if matches!(hit_ty, Type::Any) {
        hit
    } else {
        ctx.box_to_any(hit)
    };
    // `lower` may have branched; the store belongs to whatever block
    // the call actually ended in.
    let hb = ctx.cur_block;
    ctx.f
        .append_void(hb, InstKind::Store(hit_any, Operand::Value(res_slot), 0));
    ctx.f.set_term(hb, Terminator::Br(after));

    ctx.cur_block = after;
    let r = ctx.f.append_inst(
        after,
        InstKind::Load(Type::Any, Operand::Value(res_slot), 0),
        Type::Any,
        None,
    );
    Some(Operand::Value(r))
}
