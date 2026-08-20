//! `Reflect.get(target, key)` compile-time fold pulled out of
//! [`crate::ssa_lower::lower_expr_inner`] `Expr::Call` dispatch as
//! chunk-22 of the `Expr::Call` god-arm decomp (chunks 1-21 = Arr
//! higher-order + Map dispatch + Set dispatch + Arr.push + Number
//! instance methods + bare-name globals + Str regex methods + Number
//! namespace + Array.from + Arr predicate iter + Arr.flatMap +
//! Object.entries + fn-indirect + Number/String/Boolean coercion +
//! universal methods + closure-local + Object.values + Object.keys +
//! Object.getPrototypeOf + Object.assign + Bun runtime cluster).
//!
//! ES §28.1.6. A typed-struct target with a literal string key folds
//! at compile time to a field load + box-to-Any (missing key boxes to
//! ANY_UNDEF=5); everything else takes the general lane through the
//! receiver-aware [[Get]] kernel — the same one the detached call
//! rides.
//!
//! The general lane used to be a panic, which meant `Reflect.get` did
//! not compile on the shape people actually write it for:
//! `Reflect.get(obj, "a")` where `obj` is an `any` is the whole point
//! of the function, and it was a hard reject. The fold stays because
//! it is genuinely better code when it applies, not because the
//! general case is unsupported.
//!
//! The receiver (`args[2]`) is §28.1.6 step 3: it defaults to the
//! target, and it changes exactly one thing — an accessor answer runs
//! its getter against it. The typed-struct fold reads a data field, so
//! a receiver cannot change what it answers, and it stays a fold.
//! Trailing args past the receiver are spec-lowered for side-effect
//! parity (`step()`-style exprs must still fire) and discarded.
//!
//! Refcount discipline: refcounted field types `emit_rc_inc` on the
//! borrowed load before transferring ownership into the Any box —
//! caller will drop the Any box, so the underlying field needs its
//! own rc tick to stay alive.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::{LowerCtx, OBJ_HEADER_SIZE};

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let (ns_id, m_name) = match ctx.ast.get_expr(callee) {
        Expr::Member { obj, name } => (*obj, name.clone()),
        _ => return None,
    };
    if m_name != "get" {
        return None;
    }
    let Expr::Ident(ns) = ctx.ast.get_expr(ns_id) else {
        return None;
    };
    if ns != "Reflect" {
        return None;
    }
    if args.len() < 2 {
        return None;
    }
    let literal_key: Option<String> = match ctx.ast.get_expr(args[1]) {
        Expr::String(k) => Some(k.clone()),
        _ => None,
    };
    let obj_op = ctx.lower_expr(args[0]);
    let obj_ty_probe = ctx.operand_ty(&obj_op);
    // The fold needs BOTH halves: a struct layout to read out of and a
    // name to read. Anything else is the general lane.
    let (key, _) = match (literal_key, matches!(obj_ty_probe, Type::Obj(_))) {
        (Some(k), true) => (k, ()),
        _ => return Some(lower_general(ctx, args, obj_op)),
    };
    // ES §28.1.6 receiver arg + trailing silently ignored in the typed-
    // struct subset. check.rs S257 typecheck-drops them; mirror lower-
    // and-drop here so step()-style side-effect exprs fire (S272 idiom).
    // Placed after obj-lower / before dispatch so trailing eval is
    // exactly-once across the typed-struct + missing-key + panic paths.
    for &a in args.iter().skip(2) {
        let _ = ctx.lower_expr(a);
    }
    let obj_ty = ctx.operand_ty(&obj_op);
    let Type::Obj(sid) = obj_ty else {
        panic!(
            "ssa-lower: Reflect.get requires a typed-struct target with a literal string key; got target type {obj_ty:?}"
        );
    };
    let layout = ctx.struct_layouts[sid.0 as usize].clone();
    if let Some((idx, (_, fty))) = layout
        .iter()
        .enumerate()
        .find(|(_, (n, _))| n == &key)
        .map(|(i, (n, t))| (i, (n.clone(), *t)))
    {
        let offset = OBJ_HEADER_SIZE + (idx as u64) * 8;
        let cur_block = ctx.cur_block;
        let field_val =
            ctx.f
                .append_inst(cur_block, InstKind::Load(fty, obj_op, offset), fty, None);
        // borrow semantics — caller will drop the boxed Any, so rc_inc
        // refcounted fields before transferring ownership into the box.
        if fty.is_refcounted() {
            ctx.emit_rc_inc(Operand::Value(field_val));
        }
        return Some(ctx.box_to_any(Operand::Value(field_val)));
    }
    // Key absent from layout → undefined per spec.
    let cur_block = ctx.cur_block;
    let any_box = ctx.intrinsics.any_box;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(any_box, vec![Operand::ConstI64(5), Operand::ConstI64(0)]),
        Type::Any,
        None,
    );
    Some(Operand::Value(v))
}

/// The general §28.1.6 lane: strict IsObject gate on the target,
/// ToPropertyKey through the define family's shared resolver (so a
/// symbol key is a symbol, not its description), then
/// `target.[[Get]](key, receiver)`.
///
/// The kernel's answer is owned, which is what a Call expression is
/// already assumed to be, so there is nothing extra to record.
fn lower_general(ctx: &mut LowerCtx<'_>, args: &[ExprId], obj_op: Operand) -> Operand {
    let obj_ty = ctx.operand_ty(&obj_op);
    crate::ssa_lower_object_define::emit_receiver_typecheck(ctx, args[0], &obj_op, obj_ty.clone());
    let any_op = if matches!(obj_ty, Type::Any) {
        obj_op
    } else {
        ctx.box_to_any_from_expr(args[0], obj_op)
    };
    let (key_op, key_owned) = crate::ssa_lower_object_define::lower_key(
        ctx,
        &crate::ssa_lower_object_define::DefineKey::Expr(args[1]),
    );
    // §28.1.6 step 3 — an omitted receiver is the target.
    let recv_op = if args.len() >= 3 {
        let raw = ctx.lower_expr(args[2]);
        let ty = ctx.operand_ty(&raw);
        if matches!(ty, Type::Any) {
            raw
        } else {
            ctx.box_to_any_from_expr(args[2], raw)
        }
    } else {
        any_op.clone()
    };
    for &a in args.iter().skip(3) {
        let _ = ctx.lower_expr(a);
    }
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.any_member_get_with_receiver,
            vec![any_op, key_op.clone(), recv_op.clone()],
        ),
        Type::Any,
        None,
    );
    crate::ssa_lower_object_define::emit_key_release(ctx, key_op, key_owned);
    if args.len() >= 3 {
        ctx.release_owned_temp(args[2], &recv_op);
    }
    // A getter can throw, and so can ToPropertyKey.
    ctx.emit_throw_check(None);
    Operand::Value(v)
}
