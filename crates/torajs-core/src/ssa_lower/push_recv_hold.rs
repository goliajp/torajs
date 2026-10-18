//! `xs.push(arg)` where running `arg` may rebind `xs`. The receiver is
//! evaluated before the arguments (§13.3.6.1), so the append goes to
//! the array `xs` held then; but the binding's reference to it goes
//! with the rebinding, and with it possibly the array. The push holds
//! a reference of its own across the arguments and the append.
//!
//! Only an argument that can run code or assign can rebind, and only a
//! binding something can write while it runs: a top-level binding
//! assigned anywhere in the program, a local a closure shares and
//! writes (it lives in a capture box), or a local the argument itself
//! assigns.

use crate::ast::ExprId;
use crate::ssa::{Operand, Type};
use crate::ssa_lower::LowerCtx;

/// The reference a push holds on its receiver.
pub(crate) struct Hold {
    arr: Operand,
    ty: Type,
    token: usize,
}

/// Holds `arr`, read from the binding `name` (a top-level one when
/// `global`), when running `arg` may rebind it.
pub(crate) fn hold(
    ctx: &mut LowerCtx<'_>,
    name: &str,
    global: bool,
    arg: ExprId,
    arr: Operand,
) -> Option<Hold> {
    if crate::ssa_lower_push_loop_gate::expr_cannot_call(ctx, arg) {
        return None;
    }
    let written = if global {
        ctx.proofs.assigned_anywhere(ctx.ast).contains(name)
    } else {
        ctx.boxed_noncopy_lets.contains(name)
            || crate::ssa_lower_closure_captures::expr_assigned_names(ctx.ast, arg).contains(name)
    };
    if !written {
        return None;
    }
    let ty = ctx.operand_ty(&arr);
    ctx.emit_rc_inc(arr);
    // the throw edges of the arguments release it too
    let token = ctx.push_throw_temp(arr, ty);
    Some(Hold { arr, ty, token })
}

/// Releases what [`hold`] took, once the append is done.
pub(crate) fn release(ctx: &mut LowerCtx<'_>, hold: Option<Hold>) {
    if let Some(Hold { arr, ty, token }) = hold {
        ctx.pop_throw_temp(token);
        ctx.emit_drop_value(arr, ty);
    }
}
