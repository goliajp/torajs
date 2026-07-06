//! `<Array<T>>.flatMap(fn)` higher-order method pulled out of
//! [`crate::ssa_lower::lower_expr_inner`] `Expr::Call` dispatch as chunk-11
//! of the `Expr::Call` god-arm decomp (chunks 1-10 = Arr higher-order +
//! Map dispatch + Set dispatch + Arr.push + Number instance methods +
//! bare-name globals + Str regex methods + Number namespace + Array.from +
//! Arr predicate iter).
//!
//! Shape: outer loop walks the source array, per element calls `fn(elem)`
//! to get an inner Array<T>, inner loop walks that and pushes each element
//! into `dst`. Inner array's element is `rc_inc`'d before push (refcounted
//! types only) so the inner array's own drop balances correctly. dst is
//! `cap=0`; arr_push grows on demand.
//!
//! S319 — ES §23.1.3.11 spec sig is `flatMap(cb, thisArg)`; tora's
//! callbacks lack `this` binding so thisArg + any further trailing args are
//! silently dropped per S270 (check.rs already typecheck-drops args[1..]
//! for the Array callback family — this arm widens the ssa_lower gate
//! `args.len() == 1` to `>= 1` and lower-and-drops trailing for spec
//! left-to-right side-effect order).
//!
//! Returns `Some(result)` when callee matches `Expr::Member` with name
//! `flatMap` and args is non-empty; non-array receivers panic at SSA
//! lower-time (preserving the original block's behavior). `None` lets the
//! caller fall through to the next M2/M3/M4 arm below.

use crate::ast::{Expr, ExprId};
use crate::ssa::{BinOp as SsaBinOp, BlockId, IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx};

/// Try to lower a `xs.flatMap(fn, ...)` call. Returns `Some` when
/// dispatched.
pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let (obj, name) = match ctx.ast.get_expr(callee) {
        Expr::Member { obj, name } => (*obj, name.clone()),
        _ => return None,
    };
    if name != "flatMap" || args.is_empty() {
        return None;
    }
    let recv_op = ctx.lower_expr(obj);
    let recv_ty = ctx.operand_ty(&recv_op);
    let Type::Arr(_) = recv_ty else {
        panic!("ssa-lower: flatMap on non-array receiver {recv_ty:?}");
    };
    let arr_ty = recv_ty;
    let src_arr = match recv_op {
        Operand::Value(v) => v,
        _ => unreachable!(),
    };
    let fn_val = ctx.lower_expr(args[0]);
    // S319 — lower-and-drop trailing args[1..] (thisArg + any further
    // trailing) for spec left-to-right side-effect order. tora callbacks
    // lack `this` binding so thisArg is functionally dropped; check.rs
    // S270 already typecheck-drops these args.
    for &a in args.iter().skip(1) {
        let _ = ctx.lower_expr(a);
    }
    let fn_ty = ctx.operand_ty(&fn_val);
    let inner_arr_ty = match fn_ty {
        Type::FnSig(s) | Type::Closure(s) => ctx.fn_sigs[s.0 as usize].1,
        _ => panic!("flatMap callback must be callable"),
    };
    let Type::Arr(inner_arr_id) = inner_arr_ty else {
        panic!("flatMap callback must return an array");
    };
    let dst_elem_ty = ctx.arr_layouts[inner_arr_id.0 as usize];
    let dst_arr_ty = inner_arr_ty;

    // Allocate dst (cap=0; arr_push grows on demand). Chunk 625 —
    // an Any dst allocates the FLAG_ARR_ANY flavor: the walk pushes
    // NaN-box bits raw (any slots ARE box bits, so arr_push's 8B
    // store is layout-correct), but the block must self-describe as
    // Any for the kind-aware readers — the old typed alloc left an
    // unmarked block whose reads fell to the UNSET arm (undefined).
    let dst_alloc_fn = if dst_elem_ty == Type::Any {
        ctx.intrinsics.arr_alloc_any
    } else {
        ctx.intrinsics.arr_alloc
    };
    let dst_init = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(dst_alloc_fn, vec![Operand::ConstI64(0)]),
        dst_arr_ty,
        None,
    );
    let dst_slot = ctx.alloca(dst_arr_ty, Some("__fm_dst"));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(dst_init), Operand::Value(dst_slot), 0),
    );

    // Outer loop: i in 0..src.length.
    let oh = ctx.f.add_block();
    let ob = ctx.f.add_block();
    let oa = ctx.f.add_block();
    let i_slot = ctx.alloca(Type::I64, Some("__fm_i"));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::ConstI64(0), Operand::Value(i_slot), 0),
    );
    let src_len = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(src_arr), ARR_LEN_OFF),
        Type::I64,
        None,
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(oh));

    // oh: check i < src_len.
    ctx.cur_block = oh;
    let i_now = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(i_slot), 0),
        Type::I64,
        None,
    );
    let cmp = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Slt, Operand::Value(i_now), Operand::Value(src_len)),
        Type::Bool,
        None,
    );
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(cmp),
            then_blk: ob,
            else_blk: oa,
        },
    );

    // ob: load src[i], call closure(elem) → inner_arr.
    ctx.cur_block = ob;
    let src_elem_ty = ctx.arr_layouts[match arr_ty {
        Type::Arr(id) => id.0 as usize,
        _ => unreachable!(),
    }];
    // T-13.5: head-aware offset for flatMap src walk. RFC 20260707
    // chunk 625 — an Any elem reads through the kind-aware
    // borrowed-box helper (typed-behind-Arr<Any> raw slots misread
    // under a raw LoadDyn).
    let elem = if src_elem_ty == Type::Any {
        ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.arr_get_any_boxed,
                vec![Operand::Value(src_arr), Operand::Value(i_now)],
            ),
            Type::Any,
            None,
        )
    } else {
        let (off_base, off) =
            ctx.emit_arr_slot_byte_offset(Operand::Value(src_arr), Operand::Value(i_now), 3, false);
        ctx.f.append_inst(
            ctx.cur_block,
            InstKind::LoadDyn(src_elem_ty, off_base, off),
            src_elem_ty,
            None,
        )
    };
    let inner_arr = ctx.call_fn_value(fn_val, fn_ty, vec![Operand::Value(elem)]);

    emit_inner_walk(ctx, inner_arr, dst_slot, dst_arr_ty, dst_elem_ty);
    let final_dst = emit_close_and_load(
        ctx,
        inner_arr,
        inner_arr_ty,
        i_slot,
        oh,
        oa,
        dst_slot,
        dst_arr_ty,
    );
    // RFC 20260705 chunk 552 — release owned-shape temps after the
    // loop consumed them (inline arrow's minted env in the cb slot,
    // Call/New-shaped receiver).
    ctx.release_owned_temp(args[0], &fn_val);
    ctx.release_owned_temp(obj, &recv_op);
    Some(Operand::Value(final_dst))
}

/// Emit the inner-array walk (`j in 0..inner_arr.length` pushing each
/// element into `dst`). Caller must have `ctx.cur_block` pointing at the
/// outer body block (after the closure call). On exit, `ctx.cur_block`
/// points at the inner-loop after-block (the caller continues with
/// drop + outer i++).
fn emit_inner_walk(
    ctx: &mut LowerCtx<'_>,
    inner_arr: ValueId,
    dst_slot: ValueId,
    dst_arr_ty: Type,
    dst_elem_ty: Type,
) {
    let ih = ctx.f.add_block();
    let ib = ctx.f.add_block();
    let ia = ctx.f.add_block();
    let j_slot = ctx.alloca(Type::I64, Some("__fm_j"));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::ConstI64(0), Operand::Value(j_slot), 0),
    );
    let inner_len = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(inner_arr), ARR_LEN_OFF),
        Type::I64,
        None,
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(ih));
    ctx.cur_block = ih;
    let j_now = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(j_slot), 0),
        Type::I64,
        None,
    );
    let jcmp = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Slt, Operand::Value(j_now), Operand::Value(inner_len)),
        Type::Bool,
        None,
    );
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(jcmp),
            then_blk: ib,
            else_blk: ia,
        },
    );
    ctx.cur_block = ib;
    // T-13.5: head-aware offset for flatMap inner walk.
    // Chunk 625 — same kind-aware read for the cb-returned inner
    // array's Any elems (a typed array return is a typed block).
    let inner_elem = if dst_elem_ty == Type::Any {
        ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.arr_get_any_boxed,
                vec![Operand::Value(inner_arr), Operand::Value(j_now)],
            ),
            Type::Any,
            None,
        )
    } else {
        let (joff_base, joff) = ctx.emit_arr_slot_byte_offset(
            Operand::Value(inner_arr),
            Operand::Value(j_now),
            3,
            false,
        );
        ctx.f.append_inst(
            ctx.cur_block,
            InstKind::LoadDyn(dst_elem_ty, joff_base, joff),
            dst_elem_ty,
            None,
        )
    };
    if dst_elem_ty.is_refcounted() {
        ctx.emit_rc_inc(Operand::Value(inner_elem));
    }
    let cur_dst = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(dst_arr_ty, Operand::Value(dst_slot), 0),
        dst_arr_ty,
        None,
    );
    let new_dst = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.arr_push,
            vec![Operand::Value(cur_dst), Operand::Value(inner_elem)],
        ),
        dst_arr_ty,
        None,
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(new_dst), Operand::Value(dst_slot), 0),
    );
    let j_next = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::BinOp(SsaBinOp::Add, Operand::Value(j_now), Operand::ConstI64(1)),
        Type::I64,
        None,
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(j_next), Operand::Value(j_slot), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(ih));
    ctx.cur_block = ia;
}

/// Emit the inner after-block close (drop inner_arr; outer i++ + br oh)
/// then position at `oa` and load the final `dst`. Caller wraps the
/// returned `ValueId` into an `Operand::Value` for return.
#[allow(clippy::too_many_arguments)]
fn emit_close_and_load(
    ctx: &mut LowerCtx<'_>,
    inner_arr: ValueId,
    inner_arr_ty: Type,
    i_slot: ValueId,
    oh: BlockId,
    oa: BlockId,
    dst_slot: ValueId,
    dst_arr_ty: Type,
) -> ValueId {
    // Drop the inner array (its elements are now in dst, and we already
    // rc_inc'd for refcounted dst push).
    ctx.emit_drop_value(Operand::Value(inner_arr), inner_arr_ty);
    // Outer i++.
    let i_then = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(i_slot), 0),
        Type::I64,
        None,
    );
    let i_next = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::BinOp(SsaBinOp::Add, Operand::Value(i_then), Operand::ConstI64(1)),
        Type::I64,
        None,
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(i_next), Operand::Value(i_slot), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(oh));
    ctx.cur_block = oa;
    ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(dst_arr_ty, Operand::Value(dst_slot), 0),
        dst_arr_ty,
        None,
    )
}
