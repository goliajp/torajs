//! A call's own reference on a receiver or argument read out of a place.
//!
//! Receivers and non-Copy arguments go to a direct call as +0 borrows:
//! the callee neither takes nor drops a reference, so the value lives
//! only as long as the caller's owner keeps its stake. A `this`, a
//! parameter or a local binding keeps it for the whole call. A field
//! (`box.p`), an element (`xs[i]`) or a top-level binding some code
//! assigns does not: the callee, or code it runs, can overwrite that
//! place, which drops the value while the callee still reads it
//! through `this` or the parameter (`box.p!.run()` where `run` does
//! `box.p = null` read a freed cell, and the next allocation of its
//! size class answered for it). Such a read takes a reference of its
//! own before the call and drops it after; the throw edges of the
//! later arguments and of the call release it too.
//!
//! Only `Obj` values handed to a user function are held: the runtime
//! entries a direct call can reach do not run user code.

use crate::ast::{Expr, ExprId};
use crate::ssa::{FuncId, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// The references one call holds.
#[derive(Default)]
pub(crate) struct Holds(Vec<(Operand, Type, usize)>);

/// True when `callee` is a function the program defines.
pub(crate) fn user_callee(ctx: &LowerCtx<'_>, callee: FuncId) -> bool {
    ctx.fn_sig_ids.contains_key(&callee)
}

/// True when `eid` reads a place a callee can overwrite and its lowered
/// value is a borrow of that place.
fn place_read(ctx: &LowerCtx<'_>, eid: ExprId) -> bool {
    let e = crate::ssa_lower_return_borrow::peel_passthrough(ctx.ast, eid);
    match ctx.ast.get_expr(e) {
        Expr::Member { .. } | Expr::Index { .. } => !ctx.expr_owned_shape(e),
        Expr::Ident(n) => {
            !ctx.locals.contains_key(n)
                && ctx.globals.contains_key(n)
                && ctx.proofs.assigned_anywhere(ctx.ast).contains(n)
        }
        _ => false,
    }
}

impl Holds {
    /// Takes a reference on `op`, the lowered `eid`, when it is an
    /// `Obj` borrowed out of a place.
    pub(crate) fn take(&mut self, ctx: &mut LowerCtx<'_>, eid: ExprId, op: Operand) {
        let ty = ctx.operand_ty(&op);
        if !matches!(ty, Type::Obj(_)) || !place_read(ctx, eid) {
            return;
        }
        ctx.emit_rc_inc(op);
        let token = ctx.push_throw_temp(op, ty);
        self.0.push((op, ty, token));
    }

    /// Drops what [`Holds::take`] took, once the call and its throw
    /// check are done, the last taken first.
    pub(crate) fn release(self, ctx: &mut LowerCtx<'_>) {
        for (op, ty, token) in self.0.into_iter().rev() {
            ctx.pop_throw_temp(token);
            ctx.emit_drop_value(op, ty);
        }
    }
}
