//! `Array.from(iter, mapFn)` map-loop emission split out of
//! [`crate::ssa_lower_call_array_from`] (rotation 260 file-size
//! split — the thisArg thread pushed the parent to 499). Bodies
//! verbatim; the parent keeps dispatch + src materialization.

use crate::ast::{Expr, ExprId};
use crate::ssa::{BinOp as SsaBinOp, FuncId, IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx, intern_arr_layout};

/// S151 — `Array.from(iter, mapFn)` per ES §23.1.2.1. mapFn shape:
/// `(elem) → R` or `(elem, idx) → R`. Strategy: walk `src_arr_op`, per-elem
/// call mapFn, push into a pre-sized `Array<R>`. The intermediate src is
/// dropped post-loop (we own it).
/// A promoted mapFn is either the inline Closure itself or a knife-2
/// variable-routed binding — both native ABIs carry the leading
/// `__this` (missing the Ident shape shifts every loop arg).
pub(crate) fn from_mapfn_promoted(ctx: &LowerCtx<'_>, cb: ExprId) -> bool {
    match ctx.ast.get_expr(cb) {
        Expr::Closure { fn_name, .. } => ctx.ast.fnexpr_recv_fns.contains(fn_name),
        Expr::Ident(n) => ctx.ast.fnexpr_recv_locals.contains(n),
        _ => false,
    }
}

/// Knife-4 mirror (rotation 260) — a promoted fn-expr callback takes
/// the §23.1.2.1 thisArg as its leading boxed `__this` arg (undefined
/// when absent); box_to_any is a pure encoding, so an owned-shape
/// thisArg temp keeps its stake in `op` and settles after the loop,
/// same as ssa_lower_call_arr_ho.
fn lower_promoted_this(
    ctx: &mut LowerCtx<'_>,
    args: &[ExprId],
) -> (Option<Operand>, Option<(ExprId, Operand)>) {
    if !from_mapfn_promoted(ctx, args[1]) {
        return (None, None);
    }
    if let Some(&t) = args.get(2) {
        let op = ctx.lower_expr(t);
        let boxed = ctx.box_to_any_from_expr(t, op.clone());
        (Some(boxed), Some((t, op)))
    } else {
        (
            Some(Operand::Value(
                crate::ssa_lower_call_arr_ho_loop::emit_undef_any_box(ctx),
            )),
            None,
        )
    }
}

/// Pre-size dst to src.length (each iteration pushes exactly one
/// element, so capacity is exact) and answer `(src_len, dst_slot)`.
/// Chunk 625 precedent (flat_map) — an Any dst allocates the
/// FLAG_ARR_ANY flavor: push_unchecked's 8B store is layout-correct
/// (any slots ARE box bits), but the block must self-describe as Any
/// for the kind-aware readers. B1 — reserve never moves the cell;
/// the write-back is retired.
fn alloc_from_dst(
    ctx: &mut LowerCtx<'_>,
    src_arr_val: crate::ssa::ValueId,
    dst_elem_ty: Type,
    dst_arr_ty: Type,
) -> (crate::ssa::ValueId, crate::ssa::ValueId) {
    let src_len = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(src_arr_val), ARR_LEN_OFF),
        Type::I64,
        None,
    );
    let dst_alloc_fn = if dst_elem_ty == Type::Any {
        ctx.intrinsics.arr_alloc_any
    } else {
        ctx.intrinsics.arr_alloc
    };
    let dst_arr = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(dst_alloc_fn, vec![Operand::Value(src_len)]),
        dst_arr_ty,
        None,
    );
    let dst_slot = ctx.alloca(dst_arr_ty, Some("__from_dst"));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(dst_arr), Operand::Value(dst_slot), 0),
    );
    let cur_dst0 = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(dst_arr_ty, Operand::Value(dst_slot), 0),
        dst_arr_ty,
        None,
    );
    let reserved = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.arr_reserve,
            vec![Operand::Value(cur_dst0), Operand::Value(src_len)],
        ),
        dst_arr_ty,
        None,
    );
    let _ = reserved;
    (src_len, dst_slot)
}

pub(crate) fn emit_map_loop(
    ctx: &mut LowerCtx<'_>,
    src_arr_op: Operand,
    args: &[ExprId],
) -> Operand {
    let src_arr_val = match src_arr_op {
        Operand::Value(v) => v,
        _ => unreachable!("Type::Arr is always SSA Value"),
    };
    let src_arr_ty = ctx.operand_ty(&Operand::Value(src_arr_val));
    let src_elem_ty = match src_arr_ty {
        Type::Arr(id) => ctx.arr_layouts[id.0 as usize],
        _ => unreachable!(),
    };
    // Rotation 364 — an argv-face mapFn (body reads `arguments`
    // values) must not take the direct / devirt call: its reshaped
    // sig leads with the synthetic argv pointer. The loop routes it
    // through the boxed variadic dispatch (`emit_argv_face_call`)
    // with the full §23.1.2.1 «kValue, k» pair; the declared ret
    // still drives the dst layout — the variadic conv unboxes the
    // result back to it.
    let argv_face = matches!(ctx.ast.get_expr(args[1]),
        Expr::Closure { fn_name, .. } if ctx.ast.closure_argv_fns.contains(fn_name))
        || matches!(ctx.ast.get_expr(args[1]),
            Expr::Ident(n) if ctx.ast.closure_argv_locals.contains(n));
    let known_fid: Option<FuncId> = if argv_face {
        None
    } else {
        match ctx.ast.get_expr(args[1]) {
            Expr::Closure { fn_name, .. } => ctx.fn_table.get(fn_name).copied(),
            Expr::Ident(name) => ctx.fn_table.get(name).copied(),
            _ => None,
        }
    };
    let fn_val = ctx.lower_expr(args[1]);
    let fn_ty = ctx.operand_ty(&fn_val);
    let (this_arg, this_temp) = lower_promoted_this(ctx, args);
    let (sig_params, dst_elem_ty) = match fn_ty {
        Type::FnSig(s) | Type::Closure(s) => {
            let (p, r) = ctx.fn_sigs[s.0 as usize].clone();
            (p, r)
        }
        _ => {
            panic!("ssa-lower: Array.from(iter, mapFn): 2nd arg must be a function, got {fn_ty:?}")
        }
    };
    let dst_arr_id = intern_arr_layout(ctx.arr_layouts, dst_elem_ty);
    let dst_arr_ty = Type::Arr(dst_arr_id);
    let (src_len, dst_slot) = alloc_from_dst(ctx, src_arr_val, dst_elem_ty, dst_arr_ty);
    let header_blk = ctx.f.add_block();
    let body_blk = ctx.f.add_block();
    let after_blk = ctx.f.add_block();
    let i_slot = ctx.alloca(Type::I64, Some("__from_i"));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::ConstI64(0), Operand::Value(i_slot), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(header_blk));
    // header — i < src.length
    ctx.cur_block = header_blk;
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
            then_blk: body_blk,
            else_blk: after_blk,
        },
    );
    // body — elem = src[i]; mapped = mapFn(elem [, i]); dst.push(mapped); i++
    ctx.cur_block = body_blk;
    let i_body = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(i_slot), 0),
        Type::I64,
        None,
    );
    let (off_base, off) = ctx.emit_arr_slot_byte_offset(
        Operand::Value(src_arr_val),
        Operand::Value(i_body),
        3,
        false,
    );
    let elem = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::LoadDyn(src_elem_ty, off_base.clone(), off),
        src_elem_ty,
        None,
    );
    // Borrowed-read: refcounted elem types need rc_inc so mapFn's
    // parameter ownership balances against the src walker keeping its
    // slot.
    if src_elem_ty.is_refcounted() {
        ctx.emit_rc_inc(Operand::Value(elem));
    }
    // mapFn argv: [elem] or [elem, i] per sig arity. The i64 → f64
    // alignment that used to follow is one direction of the shared
    // argument contract, which the call lanes below now apply for
    // every parameter — spelling it here too would only repeat it.
    let mut call_args: Vec<Operand> = Vec::with_capacity(3);
    if let Some(t) = &this_arg {
        call_args.push(t.clone());
    }
    call_args.push(Operand::Value(elem));
    // An argv-face mapFn takes the FULL spec pair — its sig's params
    // are the synthetic argv head, not positional slots.
    if argv_face || sig_params.len() >= 2 {
        call_args.push(Operand::Value(i_body));
    }
    let sig_skip = usize::from(this_arg.is_some());
    let mapped = if argv_face {
        crate::ssa_lower_call_arr_ho_loop::emit_argv_face_call(ctx, &fn_val, fn_ty, call_args, 2)
    } else {
        match known_fid {
            Some(fid) => {
                ctx.call_fn_value_devirt(fid, fn_val.clone(), fn_ty, call_args, sig_skip, 2)
            }
            None => ctx.call_fn_value(fn_val.clone(), fn_ty, call_args, sig_skip, 2),
        }
    };
    let cur_dst = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(dst_arr_ty, Operand::Value(dst_slot), 0),
        dst_arr_ty,
        None,
    );
    let mapped_arg = ctx.raw_slot_arg(Operand::Value(mapped));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.arr_push_unchecked,
            vec![Operand::Value(cur_dst), mapped_arg],
        ),
    );
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
    ctx.f.set_term(ctx.cur_block, Terminator::Br(header_blk));
    // after — drop the intermediate src array we owned, then return dst.
    // mapFn ownership of refcounted arg was rc_inc'd above so src elements
    // stay live until src drop here.
    ctx.cur_block = after_blk;
    ctx.emit_drop_value(Operand::Value(src_arr_val), src_arr_ty);
    // RFC 20260705 chunk 552 — release an inline arrow's minted env
    // after the loop consumed it.
    ctx.release_owned_temp(args[1], &fn_val);
    if let Some((t, op)) = this_temp {
        ctx.release_owned_temp(t, &op);
    }
    let final_dst = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(dst_arr_ty, Operand::Value(dst_slot), 0),
        dst_arr_ty,
        None,
    );
    Operand::Value(final_dst)
}
