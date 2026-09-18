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
/// Object-flavor kernel. `prevent` answers the [[PreventExtensions]]
/// boolean — true for an ordinary object, the trap's verdict for a
/// Proxy (§28.1.10 step 2 reports a refusal, it does not throw).
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
    let kernel = if prevent {
        ctx.intrinsics.anyv_reflect_prevent_extensions
    } else {
        ctx.intrinsics.anyv_is_extensible
    };
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(kernel, vec![any_op]),
        Type::Bool,
        None,
    );
    // A Proxy's trap, or its §10.5.4 / §10.5.3 invariant check, throws.
    ctx.emit_throw_check(None);
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
    // An inline literal proto promotes to the dynobj lane (rotation
    // 629, the `Object.setPrototypeOf` twin).
    let proto_op = crate::ssa_lower_object_define::lower_anylane_object_arg(ctx, args[1]);
    let proto_ty = ctx.operand_ty(&proto_op);
    let proto_any = if matches!(proto_ty, Type::Any) {
        proto_op.clone()
    } else {
        ctx.box_to_any_from_expr(args[1], proto_op.clone())
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
    // The kernel borrows the proto (the link takes its own stake), so
    // an owned proto temp — the promoted literal, a fresh call product
    // — is released here, before the throw check like the Object
    // flavor does.
    ctx.release_owned_temp(args[1], &proto_op);
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
///
/// Claimed at ANY arity. §28.1.1 reads three arguments and a missing
/// one is `undefined`, which the kernel already answers for — a
/// non-callable target is step 1's TypeError, a nullish
/// argumentsList step 2's. The arity gate this site used to carry
/// sent `Reflect.apply(x)` to the generic member-call lane instead,
/// which refuses the shape outright.
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
        let op = ctx.lower_expr(*a);
        ctx.release_owned_temp(*a, &op);
    }
    // A call site that named fewer than three arguments still reaches
    // the three-operand kernel ABI; the missing ones are `undefined`,
    // which is exactly what §28.1.1 reads there — a non-callable
    // target is step 1's TypeError, a nullish argumentsList step 2's.
    // ANY_UNDEF=5 through `__torajs_anyv_box_from_pair`, the same
    // immediate every other undefined-in-an-Any-slot site mints.
    while boxed.len() < 3 {
        let cb = ctx.cur_block;
        let undef = ctx.f.append_inst(
            cb,
            InstKind::Call(
                ctx.intrinsics.any_box,
                vec![Operand::ConstI64(5), Operand::ConstI64(0)],
            ),
            Type::Any,
            None,
        );
        boxed.push(Operand::Value(undef));
    }
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.reflect_apply, boxed.clone()),
        Type::Any,
        None,
    );
    release_reflect_operands(ctx, args, &boxed);
    ctx.emit_throw_check(None);
    Operand::Value(v)
}

/// The kernels borrow their three operands (the list walk takes its
/// own +1 per element, a constructed object its own prototype stake),
/// so an owned temp among them — an array literal as the argument
/// list, a fresh call product as target or receiver — is released
/// here, after the borrow and before the throw check like the other
/// Reflect sites. Nothing released them before: `Reflect.construct(F,
/// [i])` and `Reflect.apply(f, null, [i])` each stranded one block per
/// call (14.2 MB / 14.0 MB vs 1.9 MB flat over 200k). `boxed` may be
/// longer than `args` (the undefined fills, the two-argument
/// construct form's repeated target); the zip stops at the spelled
/// operands, so a repeated target is released once.
fn release_reflect_operands(ctx: &mut LowerCtx<'_>, args: &[ExprId], boxed: &[Operand]) {
    for (&a, op) in args.iter().zip(boxed.iter()) {
        ctx.release_owned_temp(a, op);
    }
}

/// §28.1.2 Reflect.construct(target, argumentsList[, newTarget]) —
/// every operand boxes to Any for the kernel ABI; the kernel carries
/// both IsConstructor gates, the unconditional
/// CreateListFromArrayLike and the newTarget [[Prototype]] re-wire.
/// The two-argument form hands the target operand back as newTarget
/// (§28.1.2 step 2).
pub(crate) fn lower_reflect_construct(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Operand {
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
        let op = ctx.lower_expr(*a);
        ctx.release_owned_temp(*a, &op);
    }
    if boxed.len() == 2 {
        boxed.push(boxed[0].clone());
    }
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.reflect_construct, boxed.clone()),
        Type::Any,
        None,
    );
    release_reflect_operands(ctx, args, &boxed);
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
    let obj_op = crate::ssa_lower_object_define::lower_anylane_object_arg(ctx, args[0]);
    let obj_ty = ctx.operand_ty(&obj_op);
    // Rotation 549 — an owned-temp receiver stays alive across every
    // may-throw point below (receiver gate, key coerce, desc gate,
    // define kernel); park it for the throw paths.
    let recv_tok = ctx
        .throw_temp_of(args[0], &obj_op)
        .map(|(op, ty)| ctx.push_throw_temp(op, ty));
    crate::ssa_lower_object_define::emit_receiver_typecheck(ctx, args[0], &obj_op, obj_ty.clone());
    let obj_ptr: Operand = match &obj_ty {
        Type::Any => Operand::Value(ctx.any_unbox_value_as_ptr(obj_op.clone())),
        Type::Arr(_) => {
            // Kernel element writes are kind-aware — mark at the
            // reflection boundary (mirror of the Object-flavor arm).
            ctx.emit_arr_mark_kind(&obj_op);
            obj_op.clone()
        }
        // A class instance rides the same kernel dispatch as the
        // Closure receiver: §10.1.6 into its `+24` expando dict, with
        // a declared field refused there rather than shadowed here.
        Type::Closure(_) | Type::FnSig(_) | Type::Obj(_) => obj_op.clone(),
        // Every other shape whose ordinary own face IS a property bag
        // rides the same dispatch — the kernel defines into that bag,
        // and the `any` spelling of the identical receiver has always
        // reached it. `Reflect.defineProperty(new Map(), …)` answered
        // false and defined nothing while `(m as any)` answered true
        // and defined.
        t if crate::ssa_lower_index::shape_has_expando_bag(t) => obj_op.clone(),
        // What is left carries no bag to define into. `false` is the
        // honest spelling (the prop_delete Tag::Obj precedent; the
        // runtime kernel's non-dynobj arm answers the same).
        _ => {
            for a in args.iter().skip(1) {
                let _ = ctx.lower_expr(*a);
            }
            if let Some(t) = recv_tok {
                ctx.pop_throw_temp(t);
            }
            return Operand::ConstBool(false);
        }
    };
    let (key_op, key_owned) = crate::ssa_lower_object_define::lower_key(
        ctx,
        &crate::ssa_lower_object_define::DefineKey::Expr(args[1]),
    );
    let key_tok = if key_owned {
        Some(ctx.push_throw_temp(key_op.clone(), Type::Str))
    } else {
        None
    };
    // Descriptor — an inline ObjectLit materializes as a fresh dynobj
    // (owned, released after the call); everything else takes the
    // §6.2.6.5 step-1 object gate, then hands its cell to the kernel
    // (whose DescStore dispatch resolves the shape).
    let desc_eid = args[2];
    // `desc_objlit` marks a materialized inline ObjectLit (its mint
    // stake drops after the call); `desc_temp` carries a lowered
    // non-literal operand whose owned-temp stake (call / new product)
    // releases after the kernel borrow — never before.
    // `desc_tok` parks the descriptor (rotation 549): a mint (objlit
    // dynobj, as its Any bit pattern) or an owned temp, parked before
    // its own §6.2.6.5 gate and staying live across the extra-arg
    // lowers until the post-kernel drop.
    let (desc_ptr, desc_objlit, desc_temp, desc_tok): (
        Operand,
        bool,
        Option<Operand>,
        Option<usize>,
    ) = if matches!(ctx.ast.get_expr(desc_eid), Expr::ObjectLit { .. }) {
        let minted = ctx.lower_dynobj_init(desc_eid);
        let as_any = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::PtrToInt(minted.clone()),
            Type::Any,
            None,
        );
        let tok = Some(ctx.push_throw_temp(Operand::Value(as_any), Type::Any));
        (minted, true, None, tok)
    } else {
        let desc_op = ctx.lower_expr(desc_eid);
        let desc_ty = ctx.operand_ty(&desc_op);
        let tok = ctx
            .throw_temp_of(desc_eid, &desc_op)
            .map(|(o, ty)| ctx.push_throw_temp(o, ty));
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
        (ptr, false, Some(desc_op), tok)
    };
    for a in args.iter().skip(3) {
        let op = ctx.lower_expr(*a);
        ctx.release_owned_temp(*a, &op);
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
    // Unpark before the unconditional releases below — the kernel
    // throw-check after them must not drop these again.
    if let Some(t) = desc_tok {
        ctx.pop_throw_temp(t);
    }
    if desc_objlit {
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.value_drop_heap, vec![desc_ptr]),
        );
    }
    if let Some(op) = desc_temp {
        ctx.release_owned_temp(desc_eid, &op);
    }
    if let Some(t) = key_tok {
        ctx.pop_throw_temp(t);
    }
    crate::ssa_lower_object_define::emit_key_release(ctx, key_op, key_owned);
    // The receiver stays parked through the kernel check (a desc-read
    // throw fires before any resize, so the parked pointer is live);
    // its normal-path stake keeps the pre-existing accounting.
    ctx.emit_throw_check(None);
    if let Some(t) = recv_tok {
        ctx.pop_throw_temp(t);
    }
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
