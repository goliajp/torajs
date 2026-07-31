//! Reflect.* integrity-family call lowerings (rotation 266 刀
//! R2/R3) — split from `ssa_lower_call_object_integrity.rs` under
//! the 500-line file rule (the parent hit 501 when 刀 R3 landed).
//! Dispatch stays in the parent's `try_lower` Reflect branch; both
//! fns share its shape: the §10.1.6.3 strict IsObject gate (the
//! define family's receiver guard — every primitive throws, no
//! ToObject) in front of the Object-flavor kernel.

use crate::ast::ExprId;
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
