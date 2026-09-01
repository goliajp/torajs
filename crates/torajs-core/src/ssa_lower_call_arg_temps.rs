//! Call-argument temp accounting shared by the indirect call lanes
//! (`ssa_lower_call_closure_local` / `ssa_lower_call_fn_indirect`),
//! rotation 549.
//!
//! Every lane lowers its arguments in order, keeps the owned-shape
//! ones for a post-call release (args are +0 shares — chunk 569),
//! collects the temps `emit_arg_conv` mints while coercing, emits the
//! call, then throw-checks and releases. Before 549 each lane spelled
//! that as two vectors and two release loops, and none of them told
//! the throw check about the temps still alive: `f({} as any,
//! boom())` stranded the object per caught throw (175MB / 600k).
//! [`ArgTemps`] owns the pattern: [`ArgTemps::push`] snapshots an arg
//! and parks it on `temps.throw_live` when it is an owned temp, so
//! every later arg lower and the callee's own throw path drop it;
//! [`ArgTemps::check_and_release`] parks the coerce-minted temps for
//! the post-call check, runs it, unparks, and releases on the normal
//! path. (Parking the coerce temps right before the check is
//! equivalent to parking them at mint time: nothing between the mint
//! and the check can throw except the call, and the check is the
//! call's throw edge.)

use crate::ast::ExprId;
use crate::ssa::{Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) struct ArgTemps {
    owned: Vec<(ExprId, Operand)>,
    /// Fresh temps minted by `emit_arg_conv` (Any→Str conversions
    /// and the like); the callee borrows them, so they drop after.
    pub(crate) coerce: Vec<(Operand, Type)>,
    parked: Vec<usize>,
}

impl ArgTemps {
    pub(crate) fn new() -> Self {
        Self {
            owned: Vec::new(),
            coerce: Vec::new(),
            parked: Vec::new(),
        }
    }

    /// Snapshot a lowered argument for the post-call release and park
    /// it for the throw paths in between when it is an owned temp.
    pub(crate) fn push(&mut self, ctx: &mut LowerCtx<'_>, eid: ExprId, raw: Operand) {
        if let Some((op, ty)) = ctx.throw_temp_of(eid, &raw) {
            self.parked.push(ctx.push_throw_temp(op, ty));
        }
        self.owned.push((eid, raw));
    }

    /// The post-call sequence: throw-check with every arg and coerce
    /// temp parked, then unpark and release them on the normal path.
    pub(crate) fn check_and_release(mut self, ctx: &mut LowerCtx<'_>) {
        for (op, ty) in &self.coerce {
            self.parked
                .push(ctx.push_throw_temp(op.clone(), ty.clone()));
        }
        ctx.emit_throw_check(None);
        for t in self.parked {
            ctx.pop_throw_temp(t);
        }
        for (a, op) in self.owned {
            ctx.release_owned_temp(a, &op);
        }
        for (op, ty) in self.coerce {
            ctx.emit_drop_value(op, ty);
        }
    }
}
