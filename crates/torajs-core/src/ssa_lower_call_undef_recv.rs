//! The receiver of a member call, when its slot may be holding the
//! generic immortal cell.
//!
//! `r.join()` with `r` from a `find` miss is `undefined.join()`:
//! §6.2.5.5 GetValue takes the property-reference path, which is
//! `ToObject(undefined)`, which throws a TypeError (§7.1.18) — and
//! it throws BEFORE the argument list is evaluated. Every typed
//! method arm below reads the receiver as a live value of its own
//! type instead, and a pointer-shaped slot's in-band `undefined` is
//! a real address that is not an array: `r.join()` loaded a length
//! off the cell and walked it, which is a SIGSEGV, not a wrong
//! answer. 31 of the 33 array surfaces died that way, in BOTH
//! spellings — only `r.length` and `r[0]` had ever been armed.
//!
//! One site rather than one per arm because the arms number in the
//! dozens and each lowers the receiver itself: this sits in
//! `ssa_lower_call::lower`, ahead of the dispatcher cascade, and
//! hands the lowered receiver down through `redispatch_lowered` so
//! a side-effecting receiver still evaluates exactly once. It is
//! the shape [`crate::ssa_lower_call_reparent_guard`] already uses
//! at this exact position.
//!
//! Self-gating: a receiver that is not an undefable-heap source
//! returns before lowering anything, so a call on an ordinary
//! receiver is one predicate and not one instruction.

use crate::ast::{Expr, ExprId};
use crate::ssa_lower::LowerCtx;

/// Arm the catchable TypeError in front of `callee(args…)` when the
/// callee is a member read off a receiver that may be holding the
/// cell. The lowered receiver is parked for the dispatcher below,
/// so this never changes how many times the receiver is evaluated.
pub(crate) fn arm(ctx: &mut LowerCtx<'_>, callee: ExprId) {
    let Expr::Member { obj, .. } = ctx.ast.get_expr(callee) else {
        return;
    };
    let obj = *obj;
    // The any lane re-encodes the cell on its way into the box
    // (`ssa_lower_any_box`) and its kernel answers §7.1.18 already.
    if matches!(ctx.expr_types.get(&obj), Some(crate::check::Type::Any)) {
        return;
    }
    if !crate::ssa_lower_nullable_guard::is_undefable_heap_source(ctx, obj) {
        return;
    }
    let recv = ctx.lower_expr(obj);
    // The SOURCE says this value could have come from a miss; the
    // TYPE says whether this slot family spells `undefined` with the
    // cell at all. Both, or the guard fires on a family whose
    // `undefined` is a different bit pattern entirely.
    let ty = ctx.operand_ty(&recv);
    if crate::ssa_lower_nullable_guard::holds_undef_cell(ctx, obj, ty) {
        crate::ssa_lower_nullable_guard::emit_undefable_heap_guard(ctx, obj, &recv);
    }
    ctx.redispatch_lowered = Some((obj, recv));
}
