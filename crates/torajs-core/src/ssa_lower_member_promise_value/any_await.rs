//! `await` of an `any` operand.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// `await <any>`: drain microtasks (same as the typed
/// await route), hand the box to the runtime's by-VALUE dispatch,
/// propagate a rejection via the throw slot. The hook's result
/// carries its own +1 stake; an owned source temp is released here,
/// mirroring [`super::lower_promise_get_value`]'s borrow classification.
pub(crate) fn lower_any_await(ctx: &mut LowerCtx<'_>, op: Operand, obj: ExprId) -> Operand {
    let is_borrow = matches!(
        ctx.ast.get_expr(obj),
        Expr::Ident(_) | Expr::Member { .. } | Expr::Index { .. }
    );
    // the drain may rebind a borrowed operand's binding and release it
    if is_borrow {
        ctx.emit_owned_result_inc(op, Type::Any);
    }
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Call(ctx.intrinsics.microtask_drain, vec![]),
    );
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.anyv_await, vec![op]),
        Type::Any,
        None,
    );
    ctx.emit_throw_check_owned(None, op, Type::Any);
    ctx.emit_drop_value(op, Type::Any);
    Operand::Value(v)
}
