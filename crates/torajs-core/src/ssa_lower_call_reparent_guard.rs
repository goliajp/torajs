//! The re-parent guard on a typed class-method call (RFC
//! 20260920-struct-user-proto K4).
//!
//! `desugar_classes` turns `c.m(a)` into `__cm_<C>__m(c, a)` or
//! `__dispatch_m(c, a)` by name, and the typed lane then calls C's body
//! (or reads C's vtable) straight off the static type. An instance
//! re-parented with `Object.setPrototypeOf` keeps C's layout and
//! vtable, but its methods now come from its user chain. In a program
//! that can re-parent (`Ast::program_reparents`) the call tests the
//! instance's `FLAG_OBJ_USER_PROTO`: clear, the static call runs as
//! before; raised, the call goes through the any lane by name, which
//! walks the user chain. Every other program is lowered untouched.
//!
//! Shape: the receiver is lowered ONCE ahead of the split; the static
//! branch re-enters the ordinary dispatcher for the same call with
//! that operand parked in `redispatch_lowered`, so the receiver is not
//! evaluated twice. Each branch lowers the remaining arguments itself
//! — only one of them runs. The two answers meet in an entry slot.

use crate::ast::{Expr, ExprId};
use crate::ssa::{IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::LowerCtx;

/// `FLAG_OBJ_USER_PROTO` in the u32 header word at +4 (the u16 flags
/// are its high half).
const USER_PROTO_BIT: i32 = (torajs_rc::FLAG_OBJ_USER_PROTO as i32) << 16;

/// Lower call `eid` under the guard when it is a speculative
/// class-method rewrite on a class-typed receiver in a program that can
/// re-parent; `None` leaves the call to the ordinary dispatcher.
pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    if !ctx.ast.program_reparents {
        return None;
    }
    let (recv_eid, name, user_args) = method_call_shape(ctx, eid, callee, args)?;
    let recv = ctx.lower_expr(recv_eid);
    if !matches!(ctx.operand_ty(&recv), Type::Obj(_)) {
        // Not a class instance after all — hand the lowered receiver
        // to the dispatcher so it is still evaluated once.
        ctx.redispatch_lowered = Some((recv_eid, recv));
        return None;
    }
    let owned_recv = recv.clone();
    Some(split_join(
        ctx,
        &recv,
        |ctx| {
            ctx.redispatch_lowered = Some((recv_eid, recv.clone()));
            crate::ssa_lower_call::dispatch(ctx, eid, callee, args)
        },
        |ctx| {
            let boxed = ctx.box_to_any(owned_recv.clone());
            let r = crate::ssa_lower_any_method_call::emit_any_method_call(
                ctx,
                boxed,
                &name,
                Operand::ConstPtrNull,
                &user_args,
            );
            ctx.release_owned_temp(recv_eid, &owned_recv);
            ctx.emit_throw_check_owned(None, Operand::Value(r), Type::Any);
            Operand::Value(r)
        },
    ))
}

/// A typed class-getter read `obj.name` (`fid` the getter body) under
/// the same guard: a re-parented instance reads `name` through the any
/// lane, which walks its user chain. `obj_val` is already lowered.
pub(crate) fn guarded_getter(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    obj_val: Operand,
    name: &str,
    static_read: impl FnOnce(&mut LowerCtx<'_>) -> Operand,
) -> Operand {
    let recv = obj_val.clone();
    split_join(ctx, &obj_val, static_read, |ctx| {
        let boxed = ctx.box_to_any(recv);
        crate::ssa_lower_any_member::lower_any_member_read(ctx, eid, boxed, name)
    })
}

/// A typed class-setter write `obj.field = v` under the guard: a
/// re-parented instance writes through the any lane, whose set walk
/// finds its user chain's setter (or creates the own property).
/// `v` is the lowered right-hand side, borrowed.
pub(crate) fn guarded_setter(
    ctx: &mut LowerCtx<'_>,
    obj_val: Operand,
    field: &str,
    v: Operand,
    static_call: impl FnOnce(&mut LowerCtx<'_>),
) {
    let (dyn_blk, static_blk, join_blk) = split_on_user_proto(ctx, &obj_val);
    ctx.cur_block = static_blk;
    static_call(ctx);
    store_and_join(ctx, None, Operand::ConstI64(0), join_blk);
    ctx.cur_block = dyn_blk;
    // The member write takes the caller's +1 on a heap payload.
    let ty = ctx.operand_ty(&v);
    ctx.emit_owned_result_inc(v.clone(), ty);
    let (tag, val) = ctx.box_to_tag_value(v);
    let boxed = ctx.box_to_any(obj_val);
    crate::ssa_lower_assign_member_any::emit_any_member_set(
        ctx, boxed, field, tag, val, &None, false,
    );
    ctx.emit_throw_check(None);
    store_and_join(ctx, None, Operand::ConstI64(0), join_blk);
    ctx.cur_block = join_blk;
}

/// Split on `recv`'s `FLAG_OBJ_USER_PROTO`: `static_arm` lowers the
/// class's own answer, `dyn_arm` the any lane's OWNED Any, fitted to
/// the static answer's type; the two meet in an entry slot.
fn split_join(
    ctx: &mut LowerCtx<'_>,
    recv: &Operand,
    static_arm: impl FnOnce(&mut LowerCtx<'_>) -> Operand,
    dyn_arm: impl FnOnce(&mut LowerCtx<'_>) -> Operand,
) -> Operand {
    let (dyn_blk, static_blk, join_blk) = split_on_user_proto(ctx, recv);
    ctx.cur_block = static_blk;
    let static_r = static_arm(ctx);
    let ty = ctx.operand_ty(&static_r);
    let slot = (ty != Type::Void).then(|| ctx.alloca_in_entry(ty, Some("__reparent_r")));
    store_and_join(ctx, slot, static_r, join_blk);
    ctx.cur_block = dyn_blk;
    let any_r = dyn_arm(ctx);
    let fitted = fit_any_result(ctx, any_r, ty);
    store_and_join(ctx, slot, fitted, join_blk);
    ctx.cur_block = join_blk;
    match slot {
        Some(slot) => Operand::Value(ctx.f.append_inst(
            join_blk,
            InstKind::Load(ty, Operand::Value(slot), 0),
            ty,
            None,
        )),
        None => Operand::ConstI64(0),
    }
}

fn store_and_join(
    ctx: &mut LowerCtx<'_>,
    slot: Option<crate::ssa::ValueId>,
    v: Operand,
    join_blk: crate::ssa::BlockId,
) {
    if let Some(slot) = slot {
        ctx.f
            .append_void(ctx.cur_block, InstKind::Store(v, Operand::Value(slot), 0));
    }
    let cb = ctx.cur_block;
    ctx.f.set_term(cb, Terminator::Br(join_blk));
}

/// `(receiver, method name, user arguments)` of a method call on a
/// struct-typed receiver: a call `desugar_classes` rewrote by name
/// (its original shape is recorded, `cm_demote` hop 1; a `this` call
/// apart), or one it left in member form (a name several unrelated
/// classes declare).
fn method_call_shape(
    ctx: &LowerCtx<'_>,
    eid: ExprId,
    callee: ExprId,
    args: &[ExprId],
) -> Option<(ExprId, String, Vec<ExprId>)> {
    if let Some(alt) = ctx.ast.speculative_cm_rewrites.get(&eid) {
        let Expr::Call {
            callee: member,
            args: user_args,
        } = ctx.ast.get_expr(*alt)
        else {
            return None;
        };
        let Expr::Member { name, .. } = ctx.ast.get_expr(*member) else {
            return None;
        };
        return Some((*args.first()?, name.clone(), user_args.clone()));
    }
    // `this.m()` inside a method: rewritten the same way, recorded
    // apart (no demotion decision exists for it). A method reached
    // through a re-parented instance's chain still runs with that
    // instance as `this`. A super call is never recorded — it is
    // static by spec.
    if let Some(member) = ctx.ast.cm_this_static_calls.get(&eid) {
        let Expr::Member { name, .. } = ctx.ast.get_expr(*member) else {
            return None;
        };
        return Some((*args.first()?, name.clone(), args.get(1..)?.to_vec()));
    }
    let Expr::Member { obj, name } = ctx.ast.get_expr(callee) else {
        return None;
    };
    matches!(
        ctx.expr_types.get(obj),
        Some(crate::check::Type::Struct(_) | crate::check::Type::ClassRef(_))
    )
    .then(|| (*obj, name.clone(), args.to_vec()))
}

/// Branch on `FLAG_OBJ_USER_PROTO` of struct `recv`: `(raised, clear,
/// join)`. Leaves `cur_block` terminated.
fn split_on_user_proto(
    ctx: &mut LowerCtx<'_>,
    recv: &Operand,
) -> (
    crate::ssa::BlockId,
    crate::ssa::BlockId,
    crate::ssa::BlockId,
) {
    let hdr = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I32, recv.clone(), 4),
        Type::I32,
        None,
    );
    let masked = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::BinOp(
            crate::ssa::BinOp::And,
            Operand::Value(hdr),
            Operand::ConstI32(USER_PROTO_BIT),
        ),
        Type::I32,
        None,
    );
    let hit = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Ne, Operand::Value(masked), Operand::ConstI32(0)),
        Type::Bool,
        None,
    );
    let (dyn_blk, static_blk, join_blk) = (ctx.f.add_block(), ctx.f.add_block(), ctx.f.add_block());
    let cb = ctx.cur_block;
    ctx.f.set_term(
        cb,
        Terminator::CondBr {
            cond: Operand::Value(hit),
            then_blk: dyn_blk,
            else_blk: static_blk,
        },
    );
    (dyn_blk, static_blk, join_blk)
}

/// The any lane's OWNED answer `v`, fitted to the static call's result
/// type `ty` with the same ownership the static call hands back: a
/// scalar is read out and the box released, a string is materialized
/// (the box released), a heap reference keeps the box's share.
fn fit_any_result(ctx: &mut LowerCtx<'_>, v: Operand, ty: Type) -> Operand {
    match ty {
        Type::Any => v,
        Type::Void => {
            ctx.emit_drop_value(v, Type::Any);
            Operand::ConstI64(0)
        }
        Type::I64 | Type::F64 | Type::I32 => {
            let n = ctx.coerce_any_to_number(v.clone(), ty);
            ctx.emit_drop_value(v, Type::Any);
            n
        }
        Type::Bool => {
            let b = ctx.coerce_to_bool(v.clone());
            ctx.emit_drop_value(v, Type::Any);
            b
        }
        Type::Str => {
            let s = ctx.coerce_to_str(v.clone(), Type::Any);
            ctx.emit_drop_value(v, Type::Any);
            s
        }
        _ => Operand::Value(ctx.any_cell_ptr_as_ptr(v)),
    }
}
