//! S-NEW 刀 2 — lowering for `Expr::NewDynamic`, the `new` whose
//! constructor is an expression.
//!
//! The named form is bound to a `__new_<C>` factory at compile time
//! and calls it directly. Here the callee has to be evaluated first,
//! so the shape is the ordinary indirect one: box the callee, pack the
//! arguments into the same argv buffer any-receiver method calls use,
//! and let `__torajs_anyv_construct` decide whether the value is a
//! constructor at all.
//!
//! Argument ledger follows the any-method-call arms: the runtime only
//! borrows argv and the callee, so every box minted here is released
//! after the call and before the throw check, while the returned
//! object arrives owned.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_any_method_call::pack_any_argv;

pub(crate) fn lower(ctx: &mut LowerCtx<'_>, callee: ExprId, args: &[ExprId]) -> Operand {
    // Chunk-496 three-shape rule: a borrowed refcounted operand
    // rc-incs before the TRANSFER-shaped box, temps hand their
    // reference over, already-Any callees pass through borrowed.
    let callee_is_borrow = matches!(
        ctx.ast.get_expr(callee),
        Expr::Ident(_) | Expr::Member { .. }
    );
    let raw = ctx.lower_expr(callee);
    let raw_ty = ctx.operand_ty(&raw);
    let (target, target_boxed) = if raw_ty == Type::Any {
        (raw, false)
    } else {
        if callee_is_borrow && raw_ty.is_refcounted() {
            ctx.emit_rc_inc(raw.clone());
        }
        (ctx.box_to_any_from_expr(callee, raw), true)
    };

    let (argv, boxed_slots) = pack_any_argv(ctx, args);
    let cur_block = ctx.cur_block;
    let construct = ctx.intrinsics.construct;
    let result = ctx.f.append_inst(
        cur_block,
        InstKind::Call(
            construct,
            vec![
                target.clone(),
                Operand::Value(argv),
                Operand::ConstI64(args.len() as i64),
            ],
        ),
        Type::Any,
        None,
    );

    for slot in boxed_slots.into_iter().flatten() {
        ctx.emit_drop_value(slot, Type::Any);
    }
    if target_boxed {
        ctx.emit_drop_value(target, Type::Any);
    } else {
        ctx.release_owned_temp(callee, &target);
    }
    // The constructed object is owned already, so the throw path has
    // to release it.
    ctx.emit_throw_check_owned(None, Operand::Value(result), Type::Any);
    Operand::Value(result)
}
