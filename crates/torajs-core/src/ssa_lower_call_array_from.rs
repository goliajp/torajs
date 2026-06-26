//! `Array.from(iter, mapFn?)` namespace dispatch pulled out of
//! [`crate::ssa_lower::lower_expr_inner`] `Expr::Call` dispatch as
//! chunk-9 of the `Expr::Call` god-arm decomp (chunks 1-8 = Arr
//! higher-order + Map dispatch + Set dispatch + Arr.push + Number
//! instance methods + bare-name globals + Str regex methods + Number
//! namespace).
//!
//! Two phases per call:
//! 1. **src materialization** — `Array.from(s)` accepts three iter
//!    shapes per ES §23.1.2.1: string (`arr_from_string` runtime walk),
//!    typed `Array<T>` (shallow clone via `arr_slice`+per-element
//!    `rc_inc` for refcounted elems), and `Set` (delegates to
//!    `ssa_lower_arr_from_set::emit`). Other arg types panic at
//!    SSA lower-time.
//! 2. **optional mapFn loop** — `Array.from(iter, mapFn)` walks `src`,
//!    calls `mapFn(elem)` or `mapFn(elem, idx)` per arity, pushes the
//!    result into a `Array<R>` pre-sized to `src.length`. mapFn can be
//!    known FuncId (devirt) or any Type::Closure / Type::FnSig value.
//!
//! Returns `Some` when the callee shape matches `Array.from(...)` with
//! at least 1 arg; `None` otherwise so dispatch falls through to the
//! next arm.

use crate::ast::{Expr, ExprId};
use crate::ssa::{BinOp as SsaBinOp, FuncId, IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx, intern_arr_layout};

/// Try to lower `Array.from(...)`. Returns `Some` when dispatched.
pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let (ns_id, m_name) = match ctx.ast.get_expr(callee) {
        Expr::Member { obj, name } => (*obj, name.clone()),
        _ => return None,
    };
    if m_name != "from" {
        return None;
    }
    if !matches!(ctx.ast.get_expr(ns_id), Expr::Ident(n) if n == "Array") {
        return None;
    }
    if args.is_empty() {
        return None;
    }
    // S275 — widen `== 1 || == 2` to `>= 1`. The 2-arg path below handles
    // iter+mapFn; trailing args (thisArg + extras) are eval-and-dropped
    // at the end so side-effect exprs fire per ES §23.1.2.1 trailing-arg
    // ignore.
    let arg_op = ctx.lower_expr(args[0]);
    let arg_ty = ctx.operand_ty(&arg_op);
    let src_arr_op = materialize_src(ctx, arg_op, arg_ty);
    if args.len() == 1 {
        return Some(src_arr_op);
    }
    // S275 — eval-and-drop args[2..] (thisArg + any trailing) so side-
    // effect exprs fire per ES §23.1.2.1 trailing-arg ignore. tora closures
    // don't bind `this`, so thisArg value is silently discarded; args[1]
    // is the mapFn (lowered below).
    for &a in &args[2..] {
        let _ = ctx.lower_expr(a);
    }
    Some(emit_map_loop(ctx, src_arr_op, args))
}

/// Materialize `src_arr` from the iter arg per the three accepted shapes
/// (string / typed Array<T> / Set). Other input types panic at SSA
/// lower-time per ES §23.1.2.1 subset.
fn materialize_src(ctx: &mut LowerCtx<'_>, arg_op: Operand, arg_ty: Type) -> Operand {
    if matches!(arg_ty, Type::Str) {
        let arr_id = intern_arr_layout(ctx.arr_layouts, Type::Str);
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.arr_from_string, vec![arg_op]),
            Type::Arr(arr_id),
            None,
        );
        return Operand::Value(v);
    }
    if let Type::Arr(arr_id) = arg_ty {
        // S132 — `Array.from(<typed Array<T>>)` shallow copy. Same deep-
        // clone pattern as Object.values's Arr arm (arr_slice(0, len) +
        // per-element rc_inc for refcounted elem types). Bun spec
        // §23.1.2.1.
        let len = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(Type::I64, arg_op, ARR_LEN_OFF),
            Type::I64,
            None,
        );
        let cloned = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.arr_slice,
                vec![arg_op, Operand::ConstI64(0), Operand::Value(len)],
            ),
            Type::Arr(arr_id),
            None,
        );
        let elem_ty = ctx.arr_layouts[arr_id.0 as usize];
        if elem_ty.is_refcounted() {
            let cloned_len = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Load(Type::I64, Operand::Value(cloned), ARR_LEN_OFF),
                Type::I64,
                None,
            );
            ctx.emit_arr_rc_inc_range(
                Operand::Value(cloned),
                Operand::ConstI64(0),
                Operand::Value(cloned_len),
            );
        }
        return Operand::Value(cloned);
    }
    if matches!(arg_ty, Type::Set) {
        // `Array.from(set)` substrate (ES §23.1.2.1 + §24.2.3.13) shares
        // its map-iter walk with `[...set]` spread; the helper lives in
        // `ssa_lower_arr_from_set`.
        return crate::ssa_lower_arr_from_set::emit(ctx, arg_op);
    }
    panic!("ssa-lower: Array.from requires a string, Array<T>, or Set arg, got {arg_ty:?}")
}

/// S151 — `Array.from(iter, mapFn)` per ES §23.1.2.1. mapFn shape:
/// `(elem) → R` or `(elem, idx) → R`. Strategy: walk `src_arr_op`, per-elem
/// call mapFn, push into a pre-sized `Array<R>`. The intermediate src is
/// dropped post-loop (we own it).
fn emit_map_loop(ctx: &mut LowerCtx<'_>, src_arr_op: Operand, args: &[ExprId]) -> Operand {
    let src_arr_val = match src_arr_op {
        Operand::Value(v) => v,
        _ => unreachable!("Type::Arr is always SSA Value"),
    };
    let src_arr_ty = ctx.operand_ty(&Operand::Value(src_arr_val));
    let src_elem_ty = match src_arr_ty {
        Type::Arr(id) => ctx.arr_layouts[id.0 as usize],
        _ => unreachable!(),
    };
    let known_fid: Option<FuncId> = match ctx.ast.get_expr(args[1]) {
        Expr::Closure { fn_name, .. } => ctx.fn_table.get(fn_name).copied(),
        Expr::Ident(name) => ctx.fn_table.get(name).copied(),
        _ => None,
    };
    let fn_val = ctx.lower_expr(args[1]);
    let fn_ty = ctx.operand_ty(&fn_val);
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
    // Pre-size dst to src.length (each iteration pushes exactly one
    // element, so capacity is exact).
    let src_len = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(src_arr_val), ARR_LEN_OFF),
        Type::I64,
        None,
    );
    let dst_arr = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.arr_alloc, vec![Operand::Value(src_len)]),
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
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(reserved), Operand::Value(dst_slot), 0),
    );
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
    let off = ctx.emit_arr_slot_byte_offset(
        Operand::Value(src_arr_val),
        Operand::Value(i_body),
        3,
        false,
    );
    let elem = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::LoadDyn(src_elem_ty, Operand::Value(src_arr_val), off),
        src_elem_ty,
        None,
    );
    // Borrowed-read: refcounted elem types need rc_inc so mapFn's
    // parameter ownership balances against the src walker keeping its
    // slot.
    if src_elem_ty.is_refcounted() {
        ctx.emit_rc_inc(Operand::Value(elem));
    }
    // mapFn argv: [elem] or [elem, i] per sig arity. Align widths
    // (i64 → f64) for f64-typed callback params, mirroring Array.map's
    // W4 fix.
    let mut call_args: Vec<Operand> = vec![Operand::Value(elem)];
    if sig_params.len() >= 2 {
        call_args.push(Operand::Value(i_body));
    }
    for (i, a) in call_args.iter_mut().enumerate() {
        if sig_params.get(i) == Some(&Type::F64) && ctx.operand_ty(a) == Type::I64 {
            *a = ctx.coerce_to_f64(a.clone());
        }
    }
    let mapped = match known_fid {
        Some(fid) => ctx.call_fn_value_devirt(fid, fn_val.clone(), fn_ty, call_args),
        None => ctx.call_fn_value(fn_val.clone(), fn_ty, call_args),
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
    let final_dst = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(dst_arr_ty, Operand::Value(dst_slot), 0),
        dst_arr_ty,
        None,
    );
    Operand::Value(final_dst)
}
