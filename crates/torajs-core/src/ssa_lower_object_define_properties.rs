//! `Object.defineProperties(obj, props)` lowering (RFC C2) —
//! extracted from [`crate::ssa_lower_object_define`] (file-size-debt
//! trim). Compile-time unfold when `props` is an `ObjectLit` and
//! `obj` is an `Ident` (emit one `emit_define_one` per field, re-
//! reading `obj` each time so a resize in one field is seen by the
//! next). Runtime `props` (Any-dynobj-backed) routes through the
//! two-phase §20.1.2.3.1 helper (`dynobj_define_properties_from`).
//! Non-Ident receivers reuse the once-lowered operand via
//! `emit_define_one_core` — a per-field re-lower of an ObjectLit
//! receiver would mint a fresh object each time (RFC 20260713
//! chunk B: `defineProperties({}, {a: null})` must reach the per-
//! field §6.2.6.5 TypeError).

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_object_define::{
    DefineKey, emit_define_one, emit_define_one_core, emit_receiver_typecheck, is_typed_object,
};

/// Lower `Object.defineProperties(obj, props)` (RFC C2) by compile-time
/// unfold: when `props` is an `ObjectLit` and `obj` is an Ident, emit one
/// `emit_define_one` per field (re-reading `obj` each time so a resize in
/// one field is seen by the next). Returns `Some` when unfolded; `None`
/// for other shapes (runtime `props` / non-Ident obj), which the combined
/// Object-namespace no-op arm eval-drops (prior behavior).
pub(crate) fn try_lower_define_properties(
    ctx: &mut LowerCtx,
    callee_eid: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    if let Expr::Member {
        obj: ns_id,
        name: m_name,
    } = ctx.ast.get_expr(callee_eid)
        && m_name == "defineProperties"
        && let Expr::Ident(ns) = ctx.ast.get_expr(*ns_id)
        && ns == "Object"
        && args.len() == 2
    {
        // RFC C4b — spec §20.1.2.4 step 1 receiver guard. Fires for every
        // shape (incl. non-Ident / non-ObjectLit) so a primitive receiver
        // throws even when the prior fall-through to the Object no-op arm
        // would have silently eval-dropped the args.
        let obj_raw = ctx.lower_expr(args[0]);
        let obj_ty = ctx.operand_ty(&obj_raw);
        emit_receiver_typecheck(ctx, args[0], &obj_raw, obj_ty);

        // Compile-time unfold on an ObjectLit props. An Ident receiver
        // re-lowers per field (idempotent, and a resize in one field is
        // seen by the next); any other receiver shape reuses the
        // already-lowered operand — a per-field re-lower of an
        // ObjectLit receiver would mint a fresh object each time (RFC
        // 20260713 chunk B: `defineProperties({}, {a: null})` must
        // reach the per-field §6.2.6.5 TypeError).
        if let Expr::ObjectLit { fields } = ctx.ast.get_expr(args[1]) {
            // Clone the (name, desc_eid) list — `emit_define_one` borrows ctx
            // mutably, so we can't hold the AST borrow across the loop.
            let field_list: Vec<(String, ExprId)> =
                fields.iter().map(|(n, e)| (n.clone(), *e)).collect();
            let obj_is_ident = matches!(ctx.ast.get_expr(args[0]), Expr::Ident(_));
            for (name, desc_eid) in &field_list {
                if obj_is_ident {
                    emit_define_one(ctx, args[0], DefineKey::Name(name), *desc_eid);
                } else {
                    emit_define_one_core(
                        ctx,
                        obj_raw.clone(),
                        obj_ty.clone(),
                        &None,
                        DefineKey::Name(name),
                        *desc_eid,
                    );
                }
            }
        } else {
            // RFC 20260712 chunk 2 — runtime props walk: both shapes
            // Any (dynobj-backed) route through the two-phase
            // §20.1.2.3.1 helper; anything else keeps the prior
            // eval-drop (typed receivers are the RFC backlog).
            let receiver_ident: Option<String> = if let Expr::Ident(n) = ctx.ast.get_expr(args[0]) {
                Some(n.clone())
            } else {
                None
            };
            let props_op = ctx.lower_expr(args[1]);
            let props_ty = ctx.operand_ty(&props_op);
            // §20.1.2.3.1 step 2 — ToObject(Properties) throws only
            // on null / undefined (other primitives wrap to key-less
            // objects; the walk no-ops). Statically-typed non-object
            // props box once so the helper can throw; a runtime Any
            // gates before the walk (RFC 20260713 chunk B).
            if !matches!(props_ty, Type::Any) && !is_typed_object(props_ty.clone()) {
                let boxed = ctx.box_to_any_from_expr(args[1], props_op.clone());
                ctx.f.append_void(
                    ctx.cur_block,
                    InstKind::Call(ctx.intrinsics.throw_typeerror_if_props_nullish, vec![boxed]),
                );
                ctx.emit_throw_check(None);
            } else if matches!(props_ty, Type::Any) {
                ctx.f.append_void(
                    ctx.cur_block,
                    InstKind::Call(
                        ctx.intrinsics.throw_typeerror_if_props_nullish,
                        vec![props_op.clone()],
                    ),
                );
                ctx.emit_throw_check(None);
            }
            if matches!(obj_ty, Type::Any) && matches!(props_ty, Type::Any) {
                let props_ptr = ctx.any_unbox_value_as_ptr(props_op.clone());
                let dynobj = ctx.any_unbox_value_as_ptr(obj_raw.clone());
                let slot = ctx.alloca(Type::Ptr, Some("__dynobj_slot"));
                ctx.f.append_void(
                    ctx.cur_block,
                    InstKind::Store(Operand::Value(dynobj), Operand::Value(slot), 0),
                );
                ctx.f.append_void(
                    ctx.cur_block,
                    InstKind::Call(
                        ctx.intrinsics.dynobj_define_properties_from,
                        vec![Operand::Value(slot), Operand::Value(props_ptr)],
                    ),
                );
                ctx.release_owned_temp(args[1], &props_op);
                ctx.emit_throw_check(None);
                ctx.emit_any_dynobj_writeback(&receiver_ident, slot);
            } else {
                ctx.release_owned_temp(args[1], &props_op);
            }
        }
        // RFC 20260705 owned-result invariant: ES answers the receiver;
        // the pass-through result carries its own ref (this dedicated
        // path bypasses integrity's lower_noop — the 545 gate caught
        // the blanket discard over-releasing the un-inc'd receiver).
        ctx.emit_owned_result_inc(obj_raw.clone(), obj_ty);
        return Some(obj_raw);
    }
    None
}
