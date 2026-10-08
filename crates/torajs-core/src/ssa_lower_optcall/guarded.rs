//! The guarded optional call, split out of `ssa_lower_optcall`.

use super::*;

pub(super) fn lower_guarded(ctx: &mut LowerCtx<'_>, callee: ExprId, args: &[ExprId]) -> Operand {
    let transfers = ctx.expr_transfers_ownership(callee);
    let v = ctx.lower_expr(callee);
    let v_ty = ctx.operand_ty(&v);
    let res_slot = ctx.alloca_in_entry(Type::Any, Some("__optcall"));
    let nullish = emit_nullish_cond(ctx, &v, &v_ty);
    let null_blk = ctx.f.add_block();
    let hit_blk = ctx.f.add_block();
    let after = ctx.f.add_block();
    let cb = ctx.cur_block;
    ctx.f.set_term(
        cb,
        Terminator::CondBr {
            cond: nullish,
            then_blk: null_blk,
            else_blk: hit_blk,
        },
    );
    // Miss path → ANY_UNDEF box. A ptr-typed owned callee temp
    // (Nullable fn-call result) was never consumed here — release
    // its ref (dead when v is statically non-null).
    ctx.cur_block = null_blk;
    let undef_box = emit_undef_box(ctx);
    ctx.f.append_void(
        null_blk,
        InstKind::Store(undef_box, Operand::Value(res_slot), 0),
    );
    if !matches!(v_ty, Type::Any) && transfers && v_ty.is_refcounted() {
        ctx.emit_drop_value(v, v_ty);
    }
    ctx.f.set_term(null_blk, Terminator::Br(after));
    // Hit path — the arguments lower HERE (short-circuit never
    // evaluates them).
    ctx.cur_block = hit_blk;
    ctx.proofs.enter_cond();
    let (callee_av, we_boxed) = if matches!(v_ty, Type::Any) {
        (v, false)
    } else {
        // pack ledger: box_to_any is TRANSFER — a borrowed value
        // rc-incs first, an owned temp hands its ref to the box.
        if !transfers && v_ty.is_refcounted() {
            ctx.emit_rc_inc(v);
        }
        (ctx.box_to_any(v), true)
    };
    // Rotation 550 — the callee we hold (the box we minted, or an
    // owned already-Any temp released at the join) is live across
    // the arguments' lowers; park it for their throw paths.
    let callee_tok = if we_boxed {
        Some(ctx.push_throw_temp(callee_av, Type::Any))
    } else {
        ctx.park_owned_temp(callee, &v)
    };
    let packed = pack_any_argv(ctx, args);
    let argv = packed.argv;
    let result = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.any_call,
            vec![
                callee_av,
                Operand::Value(argv),
                Operand::ConstI64(args.len() as i64),
            ],
        ),
        Type::Any,
        None,
    );
    packed.release(ctx);
    ctx.unpark_owned_temp(callee_tok);
    if we_boxed {
        ctx.emit_drop_value(callee_av, Type::Any);
    }
    // result is an OWNED Any already in hand — the throw path must
    // release it (mirrors the plain method-call arm).
    ctx.emit_throw_check_owned(None, Operand::Value(result), Type::Any);
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(result), Operand::Value(res_slot), 0),
    );
    ctx.proofs.leave_cond();
    let hb = ctx.cur_block;
    ctx.f.set_term(hb, Terminator::Br(after));
    ctx.cur_block = after;
    // An Any-typed owned callee temp (member read / chained call)
    // was borrowed by the dispatch on the hit path and untouched on
    // the miss path — one release covers both.
    if matches!(v_ty, Type::Any) {
        ctx.release_owned_temp(callee, &v);
    }
    let r = ctx.f.append_inst(
        after,
        InstKind::Load(Type::Any, Operand::Value(res_slot), 0),
        Type::Any,
        None,
    );
    Operand::Value(r)
}
