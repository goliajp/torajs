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
///
/// A class method call reaches here already rewritten to
/// `__cm_C__m(receiver, …)` — an Ident callee with the receiver as
/// argument 0 — so the member test alone never saw it, and the
/// method's body was inlined straight onto the absent receiver
/// (`c.m()` on a `C | undefined` holding undefined was a SIGSEGV).
/// It is the same receiver in a different position; the guard and
/// the parking are unchanged.
pub(crate) fn arm(ctx: &mut LowerCtx<'_>, callee: ExprId, args: &[ExprId]) {
    let obj = match ctx.ast.get_expr(callee) {
        Expr::Member { obj, .. } => *obj,
        Expr::Ident(name) if crate::check::is_class_method_name(name) => match args.first() {
            Some(recv) => *recv,
            None => return,
        },
        _ => return,
    };
    // The any lane re-encodes the cell on its way into the box
    // (`ssa_lower_any_box`) and its kernel answers §7.1.18 already.
    if matches!(ctx.expr_types.get(&obj), Some(crate::check::Type::Any)) {
        return;
    }
    // A str receiver spells its absent value differently — NULL, or
    // an immortal str block whose text is "undefined" — so the generic
    // cell test above cannot see it, and without this arm
    // `s.toUpperCase()` on a `string | undefined` holding undefined
    // answered "UNDEFINED". Same position, same parking; the kernel is
    // the one the `.length` read already uses.
    if crate::ssa_lower_nullable_guard::is_nullable_str_source(ctx, obj) {
        let recv = ctx.lower_expr(obj);
        crate::ssa_lower_nullable_guard_emit::emit_nullable_str_guard(ctx, obj, &recv);
        ctx.redispatch_lowered = Some((obj, recv));
        return;
    }
    if !crate::ssa_lower_nullable_guard::is_undefable_heap_source(ctx, obj) {
        return;
    }
    let recv = ctx.lower_expr(obj);
    crate::ssa_lower_nullable_guard::emit_undef_cell_guard(ctx, obj, &recv);
    ctx.redispatch_lowered = Some((obj, recv));
}
