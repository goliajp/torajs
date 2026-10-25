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
use crate::ssa::{BlockId, IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx};

mod shape;
mod walk;
use shape::dst_shape;
use walk::{
    emit_close_and_load, emit_dynamic_push_and_close, emit_inner_walk, emit_scalar_push_and_close,
};

/// Try to lower a `xs.flatMap(fn, ...)` call. Returns `Some` when
/// dispatched.
pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
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
    // Rotation 550 — an owned receiver is live across the callback /
    // thisArg lowers and the loop's throw edges; park it.
    let recv_tok = ctx.park_owned_temp(obj, &recv_op);
    // one borrowed out of a place is held: the callback may overwrite it
    let mut holds = crate::ssa_lower::call_hold::Holds::default();
    holds.take(ctx, obj, recv_op);
    let arr_ty = recv_ty;
    let src_arr = match recv_op {
        Operand::Value(v) => v,
        _ => unreachable!(),
    };
    let fn_val = ctx.lower_expr(args[0]);
    // 550-01 — a minted callback env and an owned thisArg payload are
    // ours until the post-loop release; a callback that throws
    // mid-loop drops them.
    let fn_tok = ctx.park_owned_temp(args[0], &fn_val);
    let (this_arg, this_temp) = lower_promoted_this(ctx, args);
    let this_tok = this_temp
        .as_ref()
        .and_then(|(t, op)| ctx.park_owned_temp(*t, op));
    // Rotation 364 — an argv-face callback (body reads `arguments`
    // values, or typed slots fed `any` elements: `cb_face`) goes
    // through the boxed variadic dispatch (`emit_argv_face_call`) with
    // the full §23.1.3.13 «kValue, k, O» triple. The ret of the face
    // drives `dst_shape` — the variadic conv unboxes the result to it.
    let fn_ty = ctx.operand_ty(&fn_val);
    let (fn_ty, argv_face) =
        crate::ssa_lower_call_arr_ho::cb_face::cb_face(ctx, args[0], fn_ty, arr_ty, "flatMap", eid);
    let cb_ret_ty = match fn_ty {
        Type::FnSig(s) | Type::Closure(s) => ctx.fn_sigs[s.0 as usize].1,
        _ => panic!("flatMap callback must be callable"),
    };
    let (scalar_ret, dst_elem_ty, dst_arr_ty) = dst_shape(ctx, eid, cb_ret_ty);

    // Allocate dst (cap=0; arr_push grows on demand). Chunk 625 —
    // an Any dst allocates the FLAG_ARR_ANY flavor: the walk pushes
    // NaN-box bits raw (any slots ARE box bits, so arr_push's 8B
    // store is layout-correct), but the block must self-describe as
    // Any for the kind-aware readers — the old typed alloc left an
    // unmarked block whose reads fell to the UNSET arm (undefined).
    let OuterLoop {
        dst_slot,
        dst_tok,
        i_slot,
        i_now,
        oh,
        oa,
    } = open_outer_loop(ctx, dst_elem_ty, dst_arr_ty, src_arr);

    emit_outer_present_gate(ctx, src_arr, i_now, i_slot, oh);
    // ob: load src[i], call closure(elem) → inner_arr.
    let src_elem_ty = ctx.arr_layouts[match arr_ty {
        Type::Arr(id) => id.0 as usize,
        _ => unreachable!(),
    }];
    let elem = emit_src_elem_load(ctx, src_elem_ty, src_arr, i_now);
    // RFC 20260726-array-elem-width knife 9 — the element's width and
    // the callback parameter's width answer to two different classes,
    // so they can legitimately disagree: a named callback widened by
    // another call site still reads an i64-elem array here. That
    // conversion used to be spelled out on this side, in the one
    // direction the crash showed; it is one direction of the shared
    // argument contract, which `call_fn_value` now applies for every
    // lane. Spec §23.1.3 arity — (index, srcArray) slots are appended
    // per the callback's own declared arity (`materialize_call_args`
    // aligns the reprs, same as the ho-loop family's cb_args).
    // An argv-face callback takes the FULL spec list — its sig's
    // params are the synthetic argv head, not positional slots.
    let cb_arity = if argv_face {
        3
    } else {
        ctx.sig_param_tys(fn_ty).map_or(1, |p| p.len())
    };
    // A promoted receiver-first callback takes the boxed thisArg as
    // its leading `__this` argv entry, which is not in the sig —
    // `sig_skip` starts positional alignment after it (knife-4).
    let mut call_args: Vec<Operand> = Vec::with_capacity(4);
    if let Some(t) = &this_arg {
        call_args.push(*t);
    }
    call_args.push(Operand::Value(elem));
    if cb_arity >= 2 {
        call_args.push(Operand::Value(i_now));
    }
    if cb_arity >= 3 {
        call_args.push(Operand::Value(src_arr));
    }
    let sig_skip = usize::from(this_arg.is_some());
    let cb_ret = if argv_face {
        crate::ssa_lower_call_arr_ho_loop::emit_argv_face_call(
            ctx, &fn_val, fn_ty, call_args, sig_skip, 3,
        )
    } else {
        let r = ctx.call_fn_value(fn_val, fn_ty, call_args, sig_skip, 3);
        // 550-01 — §23.1.3.13 step 6.d ReturnIfAbrupt: a throwing
        // callback ends the iteration. This lane had no check at
        // all, so it walked the callee's ret sentinel as an array
        // (`a(i).flatMap(x => [x, boom()])` — exit 139). The argv
        // face's variadic conv carries its own check.
        ctx.emit_throw_check(None);
        r
    };

    let final_dst = if scalar_ret && cb_ret_ty == Type::Any {
        // An Any return is only decidable at runtime — ES §23.1.3.11
        // step 8's IsArray test branches between the spread walk and
        // the scalar push per element.
        emit_dynamic_push_and_close(ctx, cb_ret, i_slot, oh, oa, dst_slot, dst_arr_ty)
    } else if scalar_ret {
        // Scalar return — cb answered an owned scalar U directly (no
        // inner arr wrapping). Push it into dst as-is: the value
        // already carries the +1 the dst slot takes over (refcounted
        // types), or is an inline immediate (Number / Boolean). A
        // void callback has no result value — the pushed element is
        // the boxed-undefined sentinel.
        let push_val = scalar_push_value(ctx, cb_ret, cb_ret_ty);
        emit_scalar_push_and_close(ctx, push_val, i_slot, oh, oa, dst_slot, dst_arr_ty)
    } else {
        let inner_elem_ty = match cb_ret_ty {
            Type::Arr(id) => ctx.arr_layouts[id.0 as usize],
            _ => dst_elem_ty,
        };
        emit_inner_walk(
            ctx,
            cb_ret,
            dst_slot,
            dst_arr_ty,
            dst_elem_ty,
            inner_elem_ty,
        );
        emit_close_and_load(ctx, cb_ret, cb_ret_ty, i_slot, oh, oa, dst_slot, dst_arr_ty)
    };
    // RFC 20260705 chunk 552 — release owned-shape temps after the
    // loop consumed them (inline arrow's minted env in the cb slot,
    // Call/New-shaped receiver, the boxed thisArg's payload).
    ctx.unpark_owned_temp(dst_tok);
    holds.release(ctx);
    ctx.unpark_owned_temp(fn_tok);
    ctx.release_owned_temp(args[0], &fn_val);
    ctx.unpark_owned_temp(recv_tok);
    ctx.release_owned_temp(obj, &recv_op);
    ctx.unpark_owned_temp(this_tok);
    if let Some((t, op)) = this_temp {
        ctx.release_owned_temp(t, &op);
    }
    Some(Operand::Value(final_dst))
}

/// Rotation 286 — a promoted fn-expr callback (inline Closure in
/// `fnexpr_recv_fns`, or a knife-2 variable-routed binding in
/// `fnexpr_recv_locals`) takes the §23.1.3 thisArg (T) as its leading
/// `__this` arg: args[1] lowers into an Any box (undefined when
/// absent) — the arr_ho knife-4 protocol mirrored onto flatMap, same
/// as the predicate family (rotation 261). Non-promoted callbacks
/// answer `(None, None)`. Also lowers-and-drops the trailing args
/// (S319 — spec left-to-right side-effect order).
/// The element a scalar-returning callback contributes: its result
/// as-is (it already carries the +1 the dst slot takes over, or is an
/// immediate); the boxed-undefined sentinel for a void callback; and
/// for a view answer an owned copy, the view released as the push used
/// to consume it (rotation 468). Carved out of [`try_lower`] under the
/// 200-line function limit.
/// The outer frame `open_outer_loop` answers: the empty dst in its
/// relocation slot (with its throw-live token), the cursor slot, the
/// header's loaded cursor, and the header / after blocks the close
/// branches through.
struct OuterLoop {
    dst_slot: ValueId,
    dst_tok: Option<usize>,
    i_slot: ValueId,
    i_now: ValueId,
    oh: BlockId,
    oa: BlockId,
}

/// Allocate the dst (cap=0; arr_push grows on demand) into its slot,
/// park it for the callback's throw path (550-01 — push relocates,
/// so the pointer itself cannot be parked), and open the
/// `i in 0..src.length` header. Leaves `cur_block` at the body block.
fn open_outer_loop(
    ctx: &mut LowerCtx<'_>,
    dst_elem_ty: Type,
    dst_arr_ty: Type,
    src_arr: ValueId,
) -> OuterLoop {
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
    let dst_tok = ctx.push_throw_typed_slot(dst_slot, dst_arr_ty);

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
    ctx.cur_block = ob;
    OuterLoop {
        dst_slot,
        dst_tok,
        i_slot,
        i_now,
        oh,
        oa,
    }
}

fn scalar_push_value(
    ctx: &mut LowerCtx<'_>,
    cb_ret: crate::ssa::ValueId,
    cb_ret_ty: Type,
) -> crate::ssa::ValueId {
    if cb_ret_ty == Type::Void {
        crate::ssa_lower_call_arr_ho_loop::emit_undef_any_box(ctx)
    } else if cb_ret_ty == Type::Substr {
        let owned = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.substr_to_owned, vec![Operand::Value(cb_ret)]),
            Type::Str,
            None,
        );
        ctx.emit_drop_value(Operand::Value(cb_ret), Type::Substr);
        owned
    } else {
        cb_ret
    }
}

fn lower_promoted_this(
    ctx: &mut LowerCtx<'_>,
    args: &[ExprId],
) -> (Option<Operand>, Option<(ExprId, Operand)>) {
    let promoted = matches!(ctx.ast.get_expr(args[0]),
        Expr::Closure { fn_name, .. } if ctx.ast.fnexpr_recv_fns.contains(fn_name))
        || ctx.ast.is_fnexpr_recv_use(args[0]);
    let mut this_temp: Option<(ExprId, Operand)> = None;
    let this_arg: Option<Operand> = if promoted {
        if let Some(&t) = args.get(1) {
            let op = ctx.lower_expr(t);
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
    for &a in args.iter().skip(if promoted { 2 } else { 1 }) {
        let _ = ctx.lower_expr(a);
    }
    (this_arg, this_temp)
}

/// §23.1.3.13.1 step 3.b — FlattenIntoArray asks HasProperty before
/// it reads an index, so a hole is not mapped. The header test runs per
/// step (a callback can make holes, and each step already calls it and
/// pushes); only a cell that can have holes asks `arr_has_index`, and
/// an absent index steps straight to the next.
fn emit_outer_present_gate(
    ctx: &mut LowerCtx<'_>,
    src_arr: ValueId,
    i_now: ValueId,
    i_slot: ValueId,
    oh: BlockId,
) {
    let holey = ctx.emit_hof_sparse_probe(src_arr);
    let ask_blk = ctx.f.add_block();
    let skip_blk = ctx.f.add_block();
    let visit_blk = ctx.f.add_block();
    let cur = ctx.cur_block;
    ctx.f.set_term(
        cur,
        Terminator::CondBr {
            cond: Operand::Value(holey),
            then_blk: ask_blk,
            else_blk: visit_blk,
        },
    );
    ctx.cur_block = ask_blk;
    ctx.emit_hof_typed_gate(
        crate::ssa_lower_arr_hole_gate::TypedGate::Present,
        src_arr,
        i_now,
        skip_blk,
    );
    let cur = ctx.cur_block;
    ctx.f.set_term(cur, Terminator::Br(visit_blk));
    let i_next = ctx.f.append_inst(
        skip_blk,
        InstKind::BinOp(
            crate::ssa::BinOp::Add,
            Operand::Value(i_now),
            Operand::ConstI64(1),
        ),
        Type::I64,
        None,
    );
    ctx.f.append_void(
        skip_blk,
        InstKind::Store(Operand::Value(i_next), Operand::Value(i_slot), 0),
    );
    ctx.f.set_term(skip_blk, Terminator::Br(oh));
    ctx.cur_block = visit_blk;
}

/// T-13.5: head-aware offset for the flatMap src walk. RFC 20260707
/// chunk 625 — an Any elem reads through the kind-aware borrowed-box
/// helper (typed-behind-Arr<Any> raw slots misread under a raw
/// LoadDyn).
fn emit_src_elem_load(
    ctx: &mut LowerCtx<'_>,
    src_elem_ty: Type,
    src_arr: crate::ssa::ValueId,
    i_now: crate::ssa::ValueId,
) -> crate::ssa::ValueId {
    if src_elem_ty == Type::Any {
        crate::ssa_lower_anyv_inline::arr_get_any_borrowed(
            ctx,
            Operand::Value(src_arr),
            Operand::Value(i_now),
        )
    } else {
        let (off_base, off) =
            ctx.emit_arr_slot_byte_offset(Operand::Value(src_arr), Operand::Value(i_now), 8, false);
        ctx.f.append_inst(
            ctx.cur_block,
            InstKind::LoadDyn(src_elem_ty, off_base, off),
            src_elem_ty,
            None,
        )
    }
}
