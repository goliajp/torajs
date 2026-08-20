//! §28.1.13 `Reflect.set` lowering — split out of
//! `ssa_lower_call_reflect_integrity.rs` under the 500-line file rule
//! when the four-argument receiver form landed (the parent reached
//! 496 and the next line would have broken it).
//!
//! It sits apart from its integrity-family siblings for a reason
//! beyond arithmetic: they all hand one object to one kernel, while
//! this is the language's only [[Set]] whose lookup object and write
//! object can differ, so it is the only one that has to decide which
//! of the two rides the relocation slot.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// §28.1.13 Reflect.set(target, key, value[, receiver]) — strict IsObject gate,
/// ToPropertyKey through the define family's shared resolver (Str or
/// Symbol cell), then the [[Set]] kernel's no-throw flavor
/// (`__torajs_any_member_set_soft`): a refusal (readonly / setter-less
/// accessor / non-extensible fresh key / typed-struct no-storage)
/// answers `false`; a setter throw still propagates. The receiver
/// rides an AnyValue slot so the DynObj resize relocation writes the
/// fresh cell back (7d-A mirror of `emit_any_member_set_dyn`).
pub(crate) fn lower_reflect_set(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Operand {
    // §28.1.13 step 4 is `target.[[Set]](key, V, receiver)`: with a
    // fourth argument the property lookup and the write come apart,
    // and it is the RECEIVER that rides the relocation slot, because
    // the receiver is the object the write lands on.
    let slotted = if args.len() >= 4 { args[3] } else { args[0] };
    let receiver_ident: Option<String> = if let Expr::Ident(n) = ctx.ast.get_expr(slotted) {
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
    // A fifth argument and beyond still evaluates and is dropped
    // (§28.1.13 reads four); the fourth is the receiver and is
    // lowered, not dropped.
    let recv_any: Option<Operand> = if args.len() >= 4 {
        let raw = ctx.lower_expr(args[3]);
        let ty = ctx.operand_ty(&raw);
        Some(if matches!(ty, Type::Any) {
            raw
        } else {
            ctx.box_to_any_from_expr(args[3], raw)
        })
    } else {
        None
    };
    for a in args.iter().skip(4) {
        let _ = ctx.lower_expr(*a);
    }
    let slot = ctx.alloca(Type::Any, Some("__any_recv_slot"));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(
            recv_any.clone().unwrap_or_else(|| any_op.clone()),
            Operand::Value(slot),
            0,
        ),
    );
    let cur_block = ctx.cur_block;
    let ans = if recv_any.is_some() {
        ctx.f.append_inst(
            cur_block,
            InstKind::Call(
                ctx.intrinsics.any_member_set_with_receiver,
                vec![
                    any_op,
                    key_op.clone(),
                    Operand::Value(tag_v),
                    Operand::Value(val_v),
                    Operand::Value(slot),
                ],
            ),
            Type::I64,
            None,
        )
    } else {
        ctx.f.append_inst(
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
        )
    };
    crate::ssa_lower_object_define::emit_key_release(ctx, key_op, key_owned);
    ctx.emit_throw_check(None);
    // 7d-A writeback — an Any-typed Ident receiver re-stores its
    // local from the slot (the DynObj arm's resize relocation).
    // 7d-A writeback belongs to whichever expression the slot holds:
    // the target in the three-argument form, the receiver in the
    // four-argument one. A receiver always rides the slot as an Any,
    // and the `info.ty` test below keeps a non-Any local out of it.
    let slot_is_any = args.len() >= 4 || matches!(obj_ty, Type::Any);
    if slot_is_any
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
