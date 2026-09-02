//! `Object.assign(target, ...sources)` namespace static method pulled
//! out of [`crate::ssa_lower::lower_expr_inner`] `Expr::Call` dispatch
//! as chunk-20 of the `Expr::Call` god-arm decomp.
//!
//! S127-3 — N sources per ES §20.1.2.1. Copy each source's own
//! enumerable properties into the target, left-to-right; last-source-
//! wins emerges naturally from the per-write drop-old dance.
//!
//! RFC 20260714-objlit-accessor blade 8 — the copy walks PROPERTIES,
//! not layout slots. §20.1.2.1 step 4.c.ii reads each source key with
//! [[Get]] and writes the target with [[Set]], so an accessor on either
//! side is reached through its own half:
//!
//! * a source getter contributes what it ANSWERS (pre-fix the whole
//!   shape was rejected — the source's `__getter_v` slot made its
//!   struct type differ from the target's plain `v`);
//! * a target setter RECEIVES the value (a plain slot copy would have
//!   overwritten the setter closure);
//! * a target get-only property throws `TypeError` (ES §10.1.9) — tr
//!   used to copy the SOURCE's getter closure over it and answer the
//!   source's value, silently.
//!
//! Ownership per write, delegated to `ssa_lower_struct_own_props`:
//! a data slot takes the value's reference (a borrowed field load is
//! retained first, an owned getter result stored as-is); a setter is a
//! call, so an owned value is released after it returns.
//!
//! §20.1.2.1 is a SHALLOW copy — `Get` hands over the source's own
//! reference. tr used to deep-clone an `Arr` field (`arr_slice` +
//! per-element rc_inc), so `Object.assign(t, s); s.arr.push(x)` left
//! `t.arr` short one element where bun shares the array.
//!
//! Returns `Some(target_op)` (target passed through for chaining, per
//! spec). `None` lets the caller fall through when the callee isn't
//! `Object.assign` or `args.is_empty()`.

use crate::ast::{Expr, ExprId};
use crate::ssa::{IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_struct_own_props::{
    emit_prop_store, emit_prop_value, own_prop_sinks, own_props,
};

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let (ns_id, m_name) = match ctx.ast.get_expr(callee) {
        Expr::Member { obj, name } => (*obj, name.clone()),
        _ => return None,
    };
    if m_name != "assign" {
        return None;
    }
    let Expr::Ident(ns) = ctx.ast.get_expr(ns_id) else {
        return None;
    };
    if ns != "Object" {
        return None;
    }
    if args.is_empty() {
        return None;
    }
    let target_op = ctx.lower_expr(args[0]);
    let target_ty = ctx.operand_ty(&target_op);
    // `any` target — the checker green-lit the runtime §20.1.2.1
    // walk; one anyv_assign call per source keeps last-source-wins
    // in write order. Sources box to any (borrow shapes take +1 via
    // the transfer predicate; owned temps hand their ref to the box)
    // and release after the kernel returns — it only borrows them.
    //
    // Empty-struct target (`Object.assign({}, ...)` idiom) — the
    // struct has no slot for any prop, so the anyv_assign kernel
    // would loud-reject every write on the boxed struct receiver.
    // Alloc a fresh dynobj as the real target instead; drop the
    // just-lowered empty-struct temp. The fresh dynobj is refcnt=1
    // (owned), so the result skips the owned_result_inc that the
    // borrow-shaped `any` target arm needs.
    let empty_struct_target = matches!(
        &target_ty,
        Type::Obj(sid) if ctx.struct_layouts[sid.0 as usize].is_empty()
    );
    if matches!(target_ty, Type::Any) || empty_struct_target {
        let (walk_target, target_is_fresh) = if empty_struct_target {
            ctx.emit_drop_value(target_op, target_ty);
            let dynobj = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.dynobj_alloc, vec![]),
                Type::Ptr,
                None,
            );
            (ctx.box_to_any(Operand::Value(dynobj)), true)
        } else {
            (target_op, false)
        };
        for src_eid in &args[1..] {
            let s_raw = ctx.lower_expr(*src_eid);
            let s_ty = ctx.operand_ty(&s_raw);
            let (s_op, we_boxed) = if matches!(s_ty, Type::Any) {
                (s_raw, false)
            } else {
                if !ctx.expr_transfers_ownership(*src_eid) && s_ty.is_refcounted() {
                    ctx.emit_rc_inc(s_raw.clone());
                }
                (ctx.box_to_any(s_raw), true)
            };
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Call(
                    ctx.intrinsics.anyv_assign,
                    vec![walk_target.clone(), s_op.clone()],
                ),
            );
            if we_boxed {
                ctx.emit_drop_value(s_op, Type::Any);
            } else {
                ctx.release_owned_temp(*src_eid, &s_op);
            }
            ctx.emit_throw_check(None);
        }
        if !target_is_fresh {
            ctx.emit_owned_result_inc(walk_target.clone(), Type::Any);
        }
        return Some(walk_target);
    }
    let Type::Obj(t_sid) = target_ty else {
        panic!("ssa-lower: Object.assign target must be a struct, got {target_ty:?}");
    };
    let t_layout = ctx.struct_layouts[t_sid.0 as usize].clone();
    let sinks = own_prop_sinks(&t_layout);
    for src_eid in &args[1..] {
        let source_op = ctx.lower_expr(*src_eid);
        let src_ty = ctx.operand_ty(&source_op);
        let Type::Obj(s_sid) = src_ty else {
            panic!("ssa-lower: Object.assign source must be a struct, got {src_ty:?}");
        };
        let s_layout = ctx.struct_layouts[s_sid.0 as usize].clone();
        // §20.1.2.1 step 4.c copies the source's ENUMERABLE own keys,
        // and a `defineProperty` on the source moves that set at run
        // time — which the compile-time member list cannot carry. The
        // header test costs one not-taken branch per source; only past
        // it is each member asked whether it is hidden right now.
        let exotic = crate::ssa_lower_struct_exotic_gate::emit_exotic_field_test(ctx, &source_op);
        for prop in own_props(&s_layout, ctx.fn_sigs) {
            // The checker matched the two property sets key-for-key, so
            // a missing sink here is a compiler bug, not a user error.
            let sink = sinks
                .iter()
                .find(|s| s.key == prop.key)
                .unwrap_or_else(|| {
                    panic!(
                        "ssa-lower: Object.assign source property `{}` has no target sink",
                        prop.key.lossy()
                    )
                })
                .sink;
            let val_ty = prop.ty();
            let copy_blk = ctx.f.add_block();
            let next_blk = ctx.f.add_block();
            let probe_blk = ctx.f.add_block();
            ctx.f.set_term(
                ctx.cur_block,
                Terminator::CondBr {
                    cond: exotic.clone(),
                    then_blk: probe_blk,
                    else_blk: copy_blk,
                },
            );
            // Redefined somewhere on this source — is it THIS member?
            ctx.cur_block = probe_blk;
            let key = ctx.intern_string_literal(&prop.key);
            let hidden = ctx.f.append_inst(
                probe_blk,
                InstKind::Call(
                    ctx.intrinsics.obj_key_is_nonenumerable,
                    vec![source_op.clone(), Operand::Value(key)],
                ),
                Type::I64,
                None,
            );
            let keep = ctx.f.append_inst(
                probe_blk,
                InstKind::ICmp(IPred::Eq, Operand::Value(hidden), Operand::ConstI64(0)),
                Type::Bool,
                None,
            );
            ctx.f.set_term(
                probe_blk,
                Terminator::CondBr {
                    cond: Operand::Value(keep),
                    then_blk: copy_blk,
                    else_blk: next_blk,
                },
            );
            ctx.cur_block = copy_blk;
            let (value, owned) = emit_prop_value(ctx, &source_op, &prop);
            emit_prop_store(ctx, &target_op, &sink, value, owned, val_ty);
            let end = ctx.cur_block;
            ctx.f.set_term(end, Terminator::Br(next_blk));
            ctx.cur_block = next_blk;
        }
    }
    // RFC 20260705 owned-result invariant: ES answers the target;
    // the pass-through result carries its own ref (dedicated path,
    // bypasses integrity's lower_noop).
    ctx.emit_owned_result_inc(target_op.clone(), target_ty);
    Some(target_op)
}
