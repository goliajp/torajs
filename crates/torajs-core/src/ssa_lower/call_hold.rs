//! A call's own reference on a receiver or argument read out of a place.
//!
//! Receivers and non-Copy arguments go to a direct call as +0 borrows:
//! the callee neither takes nor drops a reference, so the value lives
//! only as long as the caller's owner keeps its stake. A `this`, a
//! parameter or a local binding no closure shares keeps it for the
//! whole call. A field (`box.p`), an element (`xs[i]`), a top-level
//! binding some code assigns or a local in a capture box does not:
//! the callee, or code it runs, can overwrite that place, which drops
//! the value while the callee still reads it through `this` or the
//! parameter (`box.p!.run()` where `run` does `box.p = null` read a
//! freed cell, and the next allocation of its size class answered for
//! it). Such a read takes a reference of its own before the call and
//! drops it after; the throw edges of the later arguments and of the
//! call release it too.
//!
//! Only class instances, strings, arrays, Maps and Sets handed to a
//! user function (or to a loop that runs a user callback over them)
//! are held: the runtime entries a direct call can reach do not run
//! user code.
//!
//! Most holds are needless: nothing that runs while the call holds the
//! value releases one that could reference the cell. Each piece of a
//! hold is emitted behind its own guard (`ssa::call_hold`) and recorded
//! in `TempScratch::holds`; once the whole module is lowered,
//! `ssa_lower_pass_2_5::call_holds` removes the ones it proves needless.

use crate::ast::{Expr, ExprId};
use crate::ssa::{CallHold, FuncId, HoldGuard, Operand, Terminator, Type};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_temp_scratch::ThrowTemp;

/// The references one call holds: the operand, its type, its throw
/// token and its index in `TempScratch::holds`.
#[derive(Default)]
pub(crate) struct Holds(Vec<(Operand, Type, usize, usize)>);

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
        // a binding shared with closures lives in a capture box, which
        // a closure the callee runs can assign
        Expr::Ident(n) if ctx.locals.contains_key(n) => ctx.boxed_noncopy_lets.contains(n),
        Expr::Ident(n) => {
            ctx.globals.contains_key(n) && ctx.proofs.assigned_anywhere(ctx.ast).contains(n)
        }
        _ => false,
    }
}

/// Emits `piece` behind a fresh guard: the current block branches to
/// it or past it, and lowering continues where the two meet.
fn guarded(ctx: &mut LowerCtx<'_>, piece: impl FnOnce(&mut LowerCtx<'_>)) -> HoldGuard {
    let at = ctx.cur_block;
    let first = ctx.f.add_block();
    ctx.cur_block = first;
    piece(ctx);
    let end = ctx.cur_block;
    let last = ctx.f.blocks.len() as u32;
    let join = ctx.f.add_block();
    ctx.f.set_term(end, Terminator::Br(join));
    ctx.f.set_term(
        at,
        Terminator::CondBr {
            cond: Operand::ConstBool(true),
            then_blk: first,
            else_blk: join,
        },
    );
    ctx.cur_block = join;
    HoldGuard {
        at,
        body: (first.0, last),
        join,
    }
}

/// Releases hold `hold`'s reference on `op` behind a guard of its own;
/// `done` marks the normal path's release, after which the hold is
/// complete.
pub(crate) fn emit_drop(ctx: &mut LowerCtx<'_>, hold: usize, op: Operand, ty: Type, done: bool) {
    let guard = guarded(ctx, |ctx| ctx.emit_drop_value(op, ty));
    let h = &mut ctx.temps.holds[hold];
    h.drops.push(guard);
    if done {
        h.done = Some(h.drops.len() - 1);
    }
}

impl Holds {
    /// Takes a reference on `op`, the lowered `eid`, when it is an
    /// `Obj`, a `Str`, an `Arr`, a `Map` or a `Set` borrowed out of a
    /// place.
    pub(crate) fn take(&mut self, ctx: &mut LowerCtx<'_>, eid: ExprId, op: Operand) {
        let ty = ctx.operand_ty(&op);
        let cell = matches!(
            ty,
            Type::Obj(_) | Type::Str | Type::Arr(_) | Type::Map | Type::Set
        );
        if !cell || !place_read(ctx, eid) {
            return;
        }
        let take = guarded(ctx, |ctx| ctx.emit_rc_inc(op));
        let hold = ctx.temps.holds.len();
        ctx.temps.holds.push(CallHold {
            cell: ty,
            take,
            drops: Vec::new(),
            done: None,
        });
        ctx.temps
            .throw_live
            .push(Some(ThrowTemp::Held(op, ty, hold)));
        let token = ctx.temps.throw_live.len() - 1;
        self.0.push((op, ty, token, hold));
    }

    /// The holds of a receiver `op` (the lowered `eid`) of a method
    /// that runs a user callback, when `runs_cb`: the callback may
    /// overwrite the place it was read out of.
    pub(crate) fn take_if(ctx: &mut LowerCtx<'_>, runs_cb: bool, eid: ExprId, op: Operand) -> Self {
        let mut holds = Self::default();
        if runs_cb {
            holds.take(ctx, eid, op);
        }
        holds
    }

    /// Drops what [`Holds::take`] took, once the call and its throw
    /// check are done, the last taken first.
    pub(crate) fn release(self, ctx: &mut LowerCtx<'_>) {
        for (op, ty, token, hold) in self.0.into_iter().rev() {
            ctx.pop_throw_temp(token);
            emit_drop(ctx, hold, op, ty, true);
        }
    }
}
