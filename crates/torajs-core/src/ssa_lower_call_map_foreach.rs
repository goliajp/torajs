//! `<Map>.forEach` lowering — the iterator loop, moved out of
//! [`crate::ssa_lower_call_map_dispatch`] whole (rotation 597) because
//! that file had run out of room under the 500-line cap and this is
//! the one family in it big enough to be worth a sibling.
//!
//! Its parent's module doc used to say `forEach` "stays inline —
//! splitting it to a helper would not reduce LOC and would force
//! threading 4 stack slots through a signature". That argument is
//! about extracting a HELPER within the same file, and it still
//! holds: nothing here is split, the whole family moved together with
//! the two loads-and-boxes helpers only it calls.

use crate::ast::{Expr, ExprId};
use crate::ssa::{FuncId, IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::LowerCtx;

/// `m.forEach(cb, ...trailing)` per spec §23.1.3.5. Iterator-loop pattern:
/// `map_iter_next` advances a sentinel cursor (-1 = start) through live
/// entries, loads (key_tag, key_val, val_tag, val_val) into stack slots,
/// re-boxes into Any pairs, then calls `cb(value, key, map)`. Devirts to
/// direct call when args[0] is a known Closure/FnDecl.
pub(crate) fn emit_map_for_each(
    ctx: &mut LowerCtx<'_>,
    recv_op: Operand,
    args: &[ExprId],
) -> Operand {
    // S316 — ES §23.1.3.5 silently ignores trailing args past cb. check.rs
    // S270 typecheck-drops them; mirror lower-and-drop here (S272 idiom).
    debug_assert!(!args.is_empty());
    // Lower the callback (Closure or FnSig). Devirt opportunity if args[0]
    // is an Expr::Closure or Ident → known FuncId.
    let known_fid: Option<FuncId> = match ctx.ast.get_expr(args[0]) {
        Expr::Closure { fn_name, .. } => ctx.fn_table.get(fn_name).copied(),
        Expr::Ident(name) if !ctx.locals.contains_key(name) => ctx.fn_table.get(name).copied(),
        _ => None,
    };
    let fn_val = ctx.lower_expr(args[0]);
    let fn_ty = ctx.operand_ty(&fn_val);
    // Knife 4 mapset mirror (arr_ho shape) — a promoted fn-expr
    // callback takes the §23.1.3.5 thisArg as its leading boxed
    // `__this` arg (undefined box when absent); the call aligns sig
    // params past it via sig_skip.
    // Rotation 597 — and a knife-2 variable-routed BINDING of the same
    // closure (`fnexpr_recv_locals`), which the four array lanes have
    // tested since rotation 286 and this pair never gained. Without it
    // `const m = new Map([[1, 7]]); m.forEach(F)` passed argv unshifted
    // to a callee whose first declared param is `__this`, so the
    // callback read its receiver off the first spec argument and its
    // own first parameter off the second: measured, it was handed the
    // KEY where the value belongs and bound `this` to the value, where
    // §23.1.3.5 says the absent thisArg is `undefined`.
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
    // Rotation 552 — the minted callback env and an owned thisArg are
    // live for the whole walk; a throwing callback leaves the loop
    // through `emit_throw_check` without reaching the releases after
    // it, so both park for that path.
    let env_tok = ctx.park_owned_temp(args[0], &fn_val);
    let this_tok = this_temp
        .as_ref()
        .and_then(|(t, op)| ctx.park_owned_temp(*t, op));

    let i_slot = ctx.alloca(Type::I64, Some("__map_iter_i"));
    // Sentinel: cursor == -1 (i64) tells runtime to start from entries[0]
    // (insertion-order walk); each call advances cursor to the next live
    // entry's index.
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::ConstI64(-1), Operand::Value(i_slot), 0),
    );
    let kt_slot = ctx.alloca(Type::I64, Some("__map_iter_kt"));
    let kv_slot = ctx.alloca(Type::I64, Some("__map_iter_kv"));
    let vt_slot = ctx.alloca(Type::I64, Some("__map_iter_vt"));
    let vv_slot = ctx.alloca(Type::I64, Some("__map_iter_vv"));

    let header_blk = ctx.f.add_block();
    let body_blk = ctx.f.add_block();
    let after_blk = ctx.f.add_block();
    ctx.f.set_term(ctx.cur_block, Terminator::Br(header_blk));

    // header — advance the iterator; exit when no more live entries.
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

    // body — re-box the (tag, payload) pairs into Any-boxes, then call
    // the closure with (value, key, map) per spec §23.1.3.6.
    ctx.cur_block = body_blk;
    let kt_v = load_i64(ctx, kt_slot);
    let kv_v = load_i64(ctx, kv_slot);
    let vt_v = load_i64(ctx, vt_slot);
    let vv_v = load_i64(ctx, vv_slot);
    let k_box = box_pair(ctx, kt_v, kv_v);
    let v_box = box_pair(ctx, vt_v, vv_v);
    // A typed `number` callback parameter takes the value through the
    // slot kernel rather than through the generic Any -> number
    // conversion the call boundary would pick: that one runs §7.1.4,
    // which turns the undefined tag into an ordinary NaN and spends
    // the F64 sentinel the parameter could have carried. `typeof v`
    // then answered "number" where `m.get(k)` on the same entry
    // answered "undefined".
    let sig_skip = usize::from(this_arg.is_some());
    let boxed_fn =
        crate::ssa_lower_call_arr_ho_loop::boxed_fn_slot(ctx, known_fid, fn_ty, sig_skip);
    let v_arg = if boxed_fn {
        Operand::Value(v_box)
    } else {
        slot_value_for_typed_param(ctx, fn_ty, this_arg.is_some(), 0, v_box)
    };
    let walk = WalkCall {
        known_fid,
        fn_val,
        fn_ty,
        this_arg,
        boxed_fn,
    };
    emit_walk_call(ctx, args[0], &walk, [v_arg, Operand::Value(k_box), recv_op]);
    // §24.1.3.9 / §24.2.3.6 step 8.a.iii ReturnIfAbrupt — a throwing
    // callback ends the walk (previously the loop swallowed it).
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

/// The callback of one Map / Set walk and how each visit calls it.
pub(crate) struct WalkCall {
    pub(crate) known_fid: Option<FuncId>,
    pub(crate) fn_val: Operand,
    pub(crate) fn_ty: Type,
    pub(crate) this_arg: Option<Operand>,
    /// A fn-typed parameter meets a box (`boxed_fn_slot`).
    pub(crate) boxed_fn: bool,
}

/// One visit's call of a Map / Set walk's callback `cb` with `spec`, the
/// (value, key, receiver) list.
pub(crate) fn emit_walk_call(ctx: &mut LowerCtx<'_>, cb: ExprId, w: &WalkCall, spec: [Operand; 3]) {
    let sig_skip = usize::from(w.this_arg.is_some());
    // Rotation 363 — an argv-face callback (collector's HOF-anon
    // arm admits `forEach` on ANY receiver spelling, these channels
    // included) must not take the direct / devirt call: its
    // reshaped sig leads with the synthetic argv pointer, and the
    // positional (value, key, map) would land in it (probe x1:
    // silent no-op loop). Route through the boxed variadic pack —
    // borrows only, the adapter's materialize incs what it stores,
    // so the direct lane's per-iteration transfer inc is skipped.
    if crate::ast::argv_face_callback::lane_takes_argv_face(ctx.ast, cb) {
        let _ = crate::ssa_lower_call_arr_ho_loop::emit_argv_face_call(
            ctx,
            &w.fn_val,
            w.fn_ty,
            spec.to_vec(),
            0,
            3,
        );
        return;
    }
    // The receiver is passed as the 3rd callback arg per spec — a
    // borrow, like the array HOF lanes pass `O` (`cb_args`): a
    // callback parameter takes no stake, and a body that keeps the
    // value incs on its own store. Rotation 552 — the per-iteration
    // "transfer" inc that stood here had no matching release
    // anywhere, so every walk that reached its callback once leaked
    // the whole receiver (387MB / 600k single-entry maps, throw or no
    // throw).
    let mut cb_args = spec.to_vec();
    if let Some(t) = &w.this_arg {
        cb_args.insert(0, *t);
    }
    let (fn_val, fn_ty) = (w.fn_val, w.fn_ty);
    // a fn-typed parameter meets a box: the boxed entry hands the call
    // to the all-`any` twin
    let _ = match w.known_fid {
        _ if w.boxed_fn => crate::ssa_lower_call_arr_ho_loop::emit_argv_face_call(
            ctx, &fn_val, fn_ty, cb_args, sig_skip, 3,
        ),
        Some(fid) => ctx.call_fn_value_devirt(fid, fn_val, fn_ty, cb_args, sig_skip, 3),
        None => ctx.call_fn_value(fn_val, fn_ty, cb_args, sig_skip, 3),
    };
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

/// The value operand a callback's user parameter `idx` takes it at.
///
/// An Any box everywhere except a declared `number` parameter, which
/// takes the F64 slot kernel — see the call site for why the generic
/// conversion is the wrong one there. Answering the box unchanged is
/// what every other parameter shape wants, so the check is one
/// signature lookup on a cold path. Shared with the Set lane, whose
/// callback takes the member twice.
pub(crate) fn slot_value_for_typed_param(
    ctx: &mut LowerCtx<'_>,
    fn_ty: Type,
    has_this: bool,
    idx: usize,
    v_box: ValueId,
) -> Operand {
    let boxed = Operand::Value(v_box);
    let skip = usize::from(has_this) + idx;
    let Some(tys) = ctx.sig_param_tys(fn_ty) else {
        return boxed;
    };
    if tys.get(skip) != Some(&Type::F64) {
        return boxed;
    }
    Operand::Value(ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.any_to_f64_slot, vec![boxed]),
        Type::F64,
        None,
    ))
}
