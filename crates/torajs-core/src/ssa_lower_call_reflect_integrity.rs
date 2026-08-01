//! Reflect.* integrity-family call lowerings (rotation 266 刀
//! R2/R3 + rotation 267 刀 R5a) — split from
//! `ssa_lower_call_object_integrity.rs` under the 500-line file rule
//! (the parent hit 501 when 刀 R3 landed). Dispatch stays in the
//! parent's `try_lower` Reflect branch; every fn shares its shape:
//! the §10.1.6.3 strict IsObject gate (the define family's receiver
//! guard — every primitive throws, no ToObject) in front of the
//! Object-flavor kernel.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// §28.1.{8,10} Reflect.isExtensible / preventExtensions — strict
/// IsObject gate (the define family's receiver guard) then the
/// Object-flavor kernel. `prevent` answers ConstBool(true): ordinary
/// [[PreventExtensions]] always succeeds, and the setter kernel's
/// un-inc'd receiver pass-through is dropped without a dec (it
/// aliases the argument box — no ref transferred).
pub(crate) fn lower_reflect_extensible(
    ctx: &mut LowerCtx<'_>,
    args: &[ExprId],
    prevent: bool,
) -> Operand {
    let obj_op = ctx.lower_expr(args[0]);
    let obj_ty = ctx.operand_ty(&obj_op);
    crate::ssa_lower_object_define::emit_receiver_typecheck(ctx, args[0], &obj_op, obj_ty.clone());
    let any_op = if matches!(obj_ty, Type::Any) {
        obj_op
    } else {
        ctx.box_to_any_from_expr(args[0], obj_op)
    };
    for a in args.iter().skip(1) {
        let _ = ctx.lower_expr(*a);
    }
    let cur_block = ctx.cur_block;
    if prevent {
        ctx.f.append_void(
            cur_block,
            InstKind::Call(ctx.intrinsics.anyv_prevent_extensions, vec![any_op]),
        );
        return Operand::ConstBool(true);
    }
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.anyv_is_extensible, vec![any_op]),
        Type::Bool,
        None,
    );
    Operand::Value(v)
}

/// §28.1.12 Reflect.setPrototypeOf(target, proto) — strict IsObject
/// gate on the target, then the boolean-answer flavor of the
/// OrdinarySetPrototypeOf core (cycle / non-extensible /
/// fixed-layout refusal = false, no throw; an invalid proto still
/// throws per step 1). Both args box to Any for the kernel ABI.
pub(crate) fn lower_reflect_set_prototype_of(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Operand {
    let obj_op = ctx.lower_expr(args[0]);
    let obj_ty = ctx.operand_ty(&obj_op);
    crate::ssa_lower_object_define::emit_receiver_typecheck(ctx, args[0], &obj_op, obj_ty.clone());
    let any_op = if matches!(obj_ty, Type::Any) {
        obj_op
    } else {
        ctx.box_to_any_from_expr(args[0], obj_op)
    };
    let proto_op = ctx.lower_expr(args[1]);
    let proto_ty = ctx.operand_ty(&proto_op);
    let proto_any = if matches!(proto_ty, Type::Any) {
        proto_op
    } else {
        ctx.box_to_any_from_expr(args[1], proto_op)
    };
    for a in args.iter().skip(2) {
        let _ = ctx.lower_expr(*a);
    }
    let cur_block = ctx.cur_block;
    let ans = ctx.f.append_inst(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.reflect_set_prototype_of,
            vec![any_op, proto_any],
        ),
        Type::I64,
        None,
    );
    ctx.emit_throw_check(None);
    let cur_block = ctx.cur_block;
    let b = ctx.f.append_inst(
        cur_block,
        InstKind::ICmp(
            crate::ssa::IPred::Ne,
            Operand::Value(ans),
            Operand::ConstI64(0),
        ),
        Type::Bool,
        None,
    );
    Operand::Value(b)
}

/// §28.1.3 Reflect.deleteProperty(target, key) — strict IsObject
/// gate, ToPropertyKey through the define family's shared resolver
/// (Str or Symbol cell), then the same OrdinaryDelete kernel the
/// `delete` expression lowers to; the i64 answer folds to Bool
/// (absent key = true, non-configurable = false per §10.1.10).
pub(crate) fn lower_reflect_delete_property(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Operand {
    let obj_op = ctx.lower_expr(args[0]);
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
    for a in args.iter().skip(2) {
        let _ = ctx.lower_expr(*a);
    }
    let cur_block = ctx.cur_block;
    let ans = ctx.f.append_inst(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.any_prop_delete_soft,
            vec![any_op, key_op.clone()],
        ),
        Type::I64,
        None,
    );
    crate::ssa_lower_object_define::emit_key_release(ctx, key_op, key_owned);
    ctx.emit_throw_check(None);
    let cur_block = ctx.cur_block;
    let b = ctx.f.append_inst(
        cur_block,
        InstKind::ICmp(
            crate::ssa::IPred::Ne,
            Operand::Value(ans),
            Operand::ConstI64(0),
        ),
        Type::Bool,
        None,
    );
    Operand::Value(b)
}

/// §28.1.1 Reflect.apply(target, thisArg, argumentsList) — every
/// operand boxes to Any for the kernel ABI; the kernel carries the
/// IsCallable gate, the no-amnesty nullish-argumentsList TypeError
/// and the shared CreateListFromArrayLike + invoke walk.
pub(crate) fn lower_reflect_apply(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Operand {
    let mut boxed = Vec::with_capacity(3);
    for &a in args.iter().take(3) {
        let op = ctx.lower_expr(a);
        let ty = ctx.operand_ty(&op);
        let any = if matches!(ty, Type::Any) {
            op
        } else {
            ctx.box_to_any_from_expr(a, op)
        };
        boxed.push(any);
    }
    for a in args.iter().skip(3) {
        let _ = ctx.lower_expr(*a);
    }
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.reflect_apply, boxed),
        Type::Any,
        None,
    );
    ctx.emit_throw_check(None);
    Operand::Value(v)
}

/// §28.1.2 Reflect.defineProperty(target, key, desc) — strict
/// IsObject gate on the target, ToPropertyKey through the define
/// family's shared resolver, then the boolean-answer flavor of the
/// runtime-descriptor define kernel: a §10.1.6.3 refusal answers
/// `false` with no throw, while a ToPropertyDescriptor throw (desc
/// not an object, getter-backed desc field, accessor/data mix) still
/// propagates through the throw-check. Every descriptor shape rides
/// the runtime `define_from_desc` walk — the Object flavor's
/// compile-time literal fast path extracts flags but calls the
/// throwing kernel, so the soft flavor skips it (a fresh ObjectLit
/// descriptor has no getters; the runtime read is observably
/// identical).
pub(crate) fn lower_reflect_define_property(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Operand {
    // Step 7d-A mirror — capture the receiver's Ident name so the
    // dynobj Any path can writeback the post-resize ptr.
    let receiver_ident: Option<String> = if let Expr::Ident(n) = ctx.ast.get_expr(args[0]) {
        Some(n.clone())
    } else {
        None
    };
    let obj_op = crate::ssa_lower_object_define::lower_define_receiver(ctx, args[0]);
    let obj_ty = ctx.operand_ty(&obj_op);
    crate::ssa_lower_object_define::emit_receiver_typecheck(ctx, args[0], &obj_op, obj_ty.clone());
    let obj_ptr: Operand = match &obj_ty {
        Type::Any => Operand::Value(ctx.any_unbox_value_as_ptr(obj_op.clone())),
        Type::Arr(_) => {
            // Kernel element writes are kind-aware — mark at the
            // reflection boundary (mirror of the Object-flavor arm).
            ctx.emit_arr_mark_kind(&obj_op);
            obj_op.clone()
        }
        Type::Closure(_) | Type::FnSig(_) => obj_op.clone(),
        // Typed Struct / Date / RegExp / ... — no expando define
        // storage yet (RFC 20260721 刀 2b backlog; mirrors the Object
        // flavor's no-op). Nothing gets defined, and `false` is the
        // honest spelling (the prop_delete Tag::Obj precedent; the
        // runtime kernel's non-dynobj arm answers the same).
        _ => {
            for a in args.iter().skip(1) {
                let _ = ctx.lower_expr(*a);
            }
            return Operand::ConstBool(false);
        }
    };
    let (key_op, key_owned) = crate::ssa_lower_object_define::lower_key(
        ctx,
        &crate::ssa_lower_object_define::DefineKey::Expr(args[1]),
    );
    // Descriptor — an inline ObjectLit materializes as a fresh dynobj
    // (owned, released after the call); everything else takes the
    // §6.2.6.5 step-1 object gate, then hands its cell to the kernel
    // (whose DescStore dispatch resolves the shape).
    let desc_eid = args[2];
    // `desc_objlit` marks a materialized inline ObjectLit (its mint
    // stake drops after the call); `desc_temp` carries a lowered
    // non-literal operand whose owned-temp stake (call / new product)
    // releases after the kernel borrow — never before.
    let (desc_ptr, desc_objlit, desc_temp): (Operand, bool, Option<Operand>) =
        if matches!(ctx.ast.get_expr(desc_eid), Expr::ObjectLit { .. }) {
            (ctx.lower_dynobj_init(desc_eid), true, None)
        } else {
            let desc_op = ctx.lower_expr(desc_eid);
            let desc_ty = ctx.operand_ty(&desc_op);
            let desc_any = if matches!(desc_ty, Type::Any) {
                desc_op.clone()
            } else {
                ctx.box_to_any_from_expr(desc_eid, desc_op.clone())
            };
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Call(
                    ctx.intrinsics.throw_typeerror_if_not_desc_object,
                    vec![desc_any.clone()],
                ),
            );
            ctx.emit_throw_check(None);
            let ptr = Operand::Value(ctx.any_unbox_value_as_ptr(desc_any));
            (ptr, false, Some(desc_op))
        };
    for a in args.iter().skip(3) {
        let _ = ctx.lower_expr(*a);
    }
    let slot = ctx.alloca(Type::Ptr, Some("__dynobj_slot"));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(obj_ptr, Operand::Value(slot), 0),
    );
    let cur_block = ctx.cur_block;
    let ans = ctx.f.append_inst(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.dynobj_define_from_desc_soft,
            vec![Operand::Value(slot), key_op.clone(), desc_ptr.clone()],
        ),
        Type::I64,
        None,
    );
    if desc_objlit {
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.value_drop_heap, vec![desc_ptr]),
        );
    }
    if let Some(op) = desc_temp {
        ctx.release_owned_temp(desc_eid, &op);
    }
    crate::ssa_lower_object_define::emit_key_release(ctx, key_op, key_owned);
    ctx.emit_throw_check(None);
    if matches!(obj_ty, Type::Any) {
        ctx.emit_any_dynobj_writeback(&receiver_ident, slot);
    }
    let cur_block = ctx.cur_block;
    let b = ctx.f.append_inst(
        cur_block,
        InstKind::ICmp(
            crate::ssa::IPred::Ne,
            Operand::Value(ans),
            Operand::ConstI64(0),
        ),
        Type::Bool,
        None,
    );
    Operand::Value(b)
}

/// §28.1.13 Reflect.set(target, key, value) — strict IsObject gate,
/// ToPropertyKey through the define family's shared resolver (Str or
/// Symbol cell), then the [[Set]] kernel's no-throw flavor
/// (`__torajs_any_member_set_soft`): a refusal (readonly / setter-less
/// accessor / non-extensible fresh key / typed-struct no-storage)
/// answers `false`; a setter throw still propagates. The receiver
/// rides an AnyValue slot so the DynObj resize relocation writes the
/// fresh cell back (7d-A mirror of `emit_any_member_set_dyn`).
pub(crate) fn lower_reflect_set(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Operand {
    let receiver_ident: Option<String> = if let Expr::Ident(n) = ctx.ast.get_expr(args[0]) {
        Some(n.clone())
    } else {
        None
    };
    let obj_op = ctx.lower_expr(args[0]);
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
    // Value — box to Any, then the owned unbox pair the kernel's
    // consume convention expects (chunk 610: unbox_value_owned fuses
    // the payload +1 the entry write takes over).
    let v_raw = ctx.lower_expr(args[2]);
    let v_ty = ctx.operand_ty(&v_raw);
    let v_any = if matches!(v_ty, Type::Any) {
        v_raw
    } else {
        ctx.box_to_any_from_expr(args[2], v_raw)
    };
    let cur_block = ctx.cur_block;
    let tag_v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.any_unbox_tag, vec![v_any.clone()]),
        Type::I64,
        None,
    );
    let cur_block = ctx.cur_block;
    let val_v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.any_unbox_value_owned, vec![v_any]),
        Type::I64,
        None,
    );
    for a in args.iter().skip(3) {
        let _ = ctx.lower_expr(*a);
    }
    let slot = ctx.alloca(Type::Any, Some("__any_recv_slot"));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(any_op, Operand::Value(slot), 0),
    );
    let cur_block = ctx.cur_block;
    let ans = ctx.f.append_inst(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.any_member_set_soft,
            vec![
                Operand::Value(slot),
                key_op.clone(),
                Operand::Value(tag_v),
                Operand::Value(val_v),
                Operand::ConstI64(-1),
            ],
        ),
        Type::I64,
        None,
    );
    crate::ssa_lower_object_define::emit_key_release(ctx, key_op, key_owned);
    ctx.emit_throw_check(None);
    // 7d-A writeback — an Any-typed Ident receiver re-stores its
    // local from the slot (the DynObj arm's resize relocation).
    if matches!(obj_ty, Type::Any)
        && let Some(name) = &receiver_ident
        && let Some(info) = ctx.locals.get(name).copied()
        && matches!(info.ty, Type::Any)
    {
        let cur_block = ctx.cur_block;
        let fresh = ctx.f.append_inst(
            cur_block,
            InstKind::Load(Type::Any, Operand::Value(slot), 0),
            Type::Any,
            None,
        );
        let cur_block = ctx.cur_block;
        ctx.f.append_void(
            cur_block,
            InstKind::Store(Operand::Value(fresh), Operand::Value(info.slot), 0),
        );
    }
    let cur_block = ctx.cur_block;
    let b = ctx.f.append_inst(
        cur_block,
        InstKind::ICmp(
            crate::ssa::IPred::Ne,
            Operand::Value(ans),
            Operand::ConstI64(0),
        ),
        Type::Bool,
        None,
    );
    Operand::Value(b)
}
