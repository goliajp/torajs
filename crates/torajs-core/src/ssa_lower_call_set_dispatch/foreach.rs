//! `<Set>.forEach` lowering — the iterator loop and the two helpers
//! only it calls, moved out of the parent whole at the file-size
//! target.

use crate::ast::{Expr, ExprId};
use crate::ssa::{FuncId, IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::LowerCtx;

/// `s.forEach(cb, ...trailing)` per spec §24.2.3.6. Callback shape is
/// `(value, value, set)` — the storage key is the Set element so we
/// re-box it twice (value side is the pinned ANY_UNDEF placeholder).
/// Iterator-loop pattern mirrors Map.forEach: `map_iter_next` advances
/// a sentinel cursor through live entries, loads (k_tag, k_val) into
/// stack slots, double-re-boxes into independent Any pairs (avoids
/// double-drop on a shared box), then calls cb with devirt routing.
pub(super) fn emit_set_for_each(
    ctx: &mut LowerCtx<'_>,
    recv_op: Operand,
    args: &[ExprId],
) -> Operand {
    // S316 — ES §24.2.3.6 silently ignores trailing args past cb.
    debug_assert!(!args.is_empty());
    let known_fid: Option<FuncId> = match ctx.ast.get_expr(args[0]) {
        Expr::Closure { fn_name, .. } => ctx.fn_table.get(fn_name).copied(),
        Expr::Ident(name) if !ctx.locals.contains_key(name) => ctx.fn_table.get(name).copied(),
        _ => None,
    };
    let fn_val = ctx.lower_expr(args[0]);
    let fn_ty = ctx.operand_ty(&fn_val);
    // Knife 4 mapset mirror — promoted fn-expr callback takes the
    // §24.2.3.6 thisArg as its leading boxed `__this` arg (see
    // Map.forEach above).
    // Rotation 597 — and a knife-2 variable-routed BINDING of the same
    // closure (`fnexpr_recv_locals`), which the four array lanes have
    // tested since rotation 286 and this pair never gained. Without it
    // `const s = new Set([7]); s.forEach(F)` passed argv unshifted to a
    // callee whose first declared param is `__this`, so the callback
    // read its receiver off the first spec argument and its own first
    // parameter off the second: measured, Map.forEach handed the
    // callback the KEY where the value belongs and bound `this` to the
    // value, where the spec says the absent thisArg is `undefined`.
    let promoted = matches!(ctx.ast.get_expr(args[0]),
        Expr::Closure { fn_name, .. } if ctx.ast.fnexpr_recv_fns.contains(fn_name))
        || ctx.ast.is_fnexpr_recv_use(args[0]);
    let mut this_temp: Option<(ExprId, Operand)> = None;
    let this_arg: Option<Operand> = if promoted {
        if let Some(&t) = args.get(1) {
            let op = ctx.lower_expr(t);
            // box_to_any is a pure encoding — an owned-shape thisArg
            // temp keeps its stake in `op`; release after the loop
            // (arr_ho mirror), else iteration 2 reads freed memory.
            let boxed = ctx.box_to_any_from_expr(t, op);
            this_temp = Some((t, op));
            Some(boxed)
        } else {
            Some(Operand::Value(
                crate::ssa_lower_call_arr_ho_loop::emit_undef_any_box(ctx),
            ))
        }
    } else {
        None
    };
    // S316 — trailing args lower after cb-lower so eval order is
    // cb → trailing → loop. A promoted callback's thisArg lowered
    // above.
    for &a in args.iter().skip(if promoted { 2 } else { 1 }) {
        let _ = ctx.lower_expr(a);
    }
    // Rotation 552 — env and owned thisArg park for the callback's
    // throw path (Map.forEach mirror).
    let env_tok = ctx.park_owned_temp(args[0], &fn_val);
    let this_tok = this_temp
        .as_ref()
        .and_then(|(t, op)| ctx.park_owned_temp(*t, op));

    let i_slot = ctx.alloca(Type::I64, Some("__set_iter_i"));
    // Sentinel for runtime-side iterator (see Map.forEach above).
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::ConstI64(-1), Operand::Value(i_slot), 0),
    );
    let kt_slot = ctx.alloca(Type::I64, Some("__set_iter_kt"));
    let kv_slot = ctx.alloca(Type::I64, Some("__set_iter_kv"));
    let vt_slot = ctx.alloca(Type::I64, Some("__set_iter_vt"));
    let vv_slot = ctx.alloca(Type::I64, Some("__set_iter_vv"));

    let header_blk = ctx.f.add_block();
    let body_blk = ctx.f.add_block();
    let after_blk = ctx.f.add_block();
    ctx.f.set_term(ctx.cur_block, Terminator::Br(header_blk));

    ctx.cur_block = header_blk;
    let live = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.map_iter_next,
            vec![
                recv_op,
                Operand::Value(i_slot),
                Operand::Value(kt_slot),
                Operand::Value(kv_slot),
                Operand::Value(vt_slot),
                Operand::Value(vv_slot),
            ],
        ),
        Type::I64,
        None,
    );
    let cond = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Ne, Operand::Value(live), Operand::ConstI64(0)),
        Type::Bool,
        None,
    );
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(cond),
            then_blk: body_blk,
            else_blk: after_blk,
        },
    );

    ctx.cur_block = body_blk;
    let kt_v = load_i64(ctx, kt_slot);
    let kv_v = load_i64(ctx, kv_slot);
    // Re-box twice so each closure arg is an independently-owned Any-box
    // (avoids double-drop on a shared box).
    let v_box1 = box_pair(ctx, kt_v, kv_v);
    let v_box2 = box_pair(ctx, kt_v, kv_v);
    // §24.2.3.6 hands the member as BOTH the value and the key, so a
    // declared `number` parameter takes each through the slot kernel
    // (Map.forEach's twin: the generic conversion runs §7.1.4 and
    // spends the F64 sentinel for an ordinary NaN).
    let has_this = this_arg.is_some();
    let sig_skip = usize::from(has_this);
    let boxed_fn =
        crate::ssa_lower_call_arr_ho_loop::boxed_fn_slot(ctx, known_fid, fn_ty, sig_skip);
    let (a0, a1) = if boxed_fn {
        (Operand::Value(v_box1), Operand::Value(v_box2))
    } else {
        (
            crate::ssa_lower_call_map_foreach::slot_value_for_typed_param(
                ctx, fn_ty, has_this, 0, v_box1,
            ),
            crate::ssa_lower_call_map_foreach::slot_value_for_typed_param(
                ctx, fn_ty, has_this, 1, v_box2,
            ),
        )
    };
    // the call itself is Map.forEach's (argv face, boxed entry, devirt)
    let walk = crate::ssa_lower_call_map_foreach::WalkCall {
        known_fid,
        fn_val,
        fn_ty,
        this_arg,
        boxed_fn,
    };
    crate::ssa_lower_call_map_foreach::emit_walk_call(ctx, args[0], &walk, [a0, a1, recv_op]);
    // §24.2.3.6 step 8.a.iii ReturnIfAbrupt — a throwing callback
    // ends the walk (previously the loop swallowed it).
    ctx.emit_throw_check(known_fid);
    ctx.f.set_term(ctx.cur_block, Terminator::Br(header_blk));

    ctx.cur_block = after_blk;
    // RFC 20260705 chunk 552 — release an inline arrow's minted env
    // after the loop consumed it.
    ctx.unpark_owned_temp(this_tok);
    ctx.unpark_owned_temp(env_tok);
    ctx.release_owned_temp(args[0], &fn_val);
    if let Some((t, op)) = this_temp {
        ctx.release_owned_temp(t, &op);
    }
    Operand::ConstI64(0)
}

fn load_i64(ctx: &mut LowerCtx<'_>, slot: ValueId) -> ValueId {
    ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(slot), 0),
        Type::I64,
        None,
    )
}

fn box_pair(ctx: &mut LowerCtx<'_>, tag: ValueId, val: ValueId) -> ValueId {
    ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.any_box,
            vec![Operand::Value(tag), Operand::Value(val)],
        ),
        Type::Any,
        None,
    )
}
