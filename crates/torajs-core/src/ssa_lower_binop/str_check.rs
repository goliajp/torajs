//! The throw check behind a string `+`: a concat whose result would
//! pass the longest string records a RangeError and answers its left
//! operand (`torajs-str` `max_len`), and a `catch` around it has to
//! see that.
//!
//! The check is emitted only where the function has a `try` open
//! around the concat. Every string-building loop in a hot path pays
//! one flag load and branch per `+` otherwise, and outside a `try` the
//! pending RangeError still reaches the caller through the next call
//! that checks for one.
//!
//! Where the check goes is the rest of the question. Right after the
//! concat is correct, and is what every `+` gets, including each `+`
//! of a chain: in `a + b + f()` a failed `a + b` must not go on to
//! call `f`. Only `s = s + x` and `s += x` cannot take it there: the
//! assignment releases the old `s` right behind the concat, and
//! `torajs-egraph`'s `str_append` turns that concat-then-release pair
//! into an append that grows `s` in place, which only happens when
//! nothing sits between the two. So that one `+` defers its check to
//! the assignment, which runs it once the new value is stored. The
//! kernel's answer on failure is the left operand, which is `s`
//! itself, so the stored value is the binding's old one and a `catch`
//! reads `s` as it was. A longer chain `s = s + a + b` does not defer:
//! its last `+` joins onto `s + a`, not onto `s`.

use crate::ast::{BinOp, Expr, ExprId};
use crate::ssa::Operand;
use crate::ssa_lower::LowerCtx;

/// The bookkeeping between the `+` that may raise and the place its
/// check is emitted.
#[derive(Default)]
pub(crate) struct StrConcatCheck {
    /// The `+` whose check the enclosing assignment emits; taken by
    /// that `+` when its lowering starts.
    defer_root: Option<ExprId>,
    /// The deferred `+` emitted a concat; the assignment owes a check.
    pending: bool,
    /// The `+` being lowered emitted a concat kernel call.
    pub(crate) emitted: bool,
}

/// Whether a concat lowered here checks for its RangeError: only
/// under an open `try` of this function, whose `catch` the check
/// branches to.
pub(crate) fn checks_here(ctx: &LowerCtx<'_>) -> bool {
    !ctx.try_stack.is_empty()
}

/// Before an assignment to `name` lowers its right side: a `+` there
/// whose left operand is `name` itself leaves its check to
/// [`emit_deferred`]. Any other right side checks before the store,
/// so a failed `s = t + u` leaves `s` alone rather than storing `t`.
pub(crate) fn defer_rhs(ctx: &mut LowerCtx<'_>, name: &str, value: ExprId) {
    if let Expr::BinOp {
        op: BinOp::Add,
        left,
        ..
    } = ctx.ast.get_expr(value)
        && matches!(ctx.ast.get_expr(*left), Expr::Ident(n) if n == name)
    {
        ctx.binop.str_check.defer_root = Some(value);
    }
}

/// After the assignment stored its value: the check its `+` left
/// behind.
pub(crate) fn emit_deferred(ctx: &mut LowerCtx<'_>) {
    ctx.binop.str_check.defer_root = None;
    if std::mem::take(&mut ctx.binop.str_check.pending) && checks_here(ctx) {
        ctx.emit_throw_check(None);
    }
}

/// At the start of `eid`'s lowering: whether its check is deferred.
/// Anything its operands lower runs with no deferral armed.
pub(super) fn take_root(ctx: &mut LowerCtx<'_>, eid: ExprId) -> bool {
    ctx.binop.str_check.defer_root.take() == Some(eid)
}

/// After the `+` dropped its operands: the check, or the note that
/// the assignment it deferred to owes it. On the throw edge the
/// result dies with the expression that was computing it.
pub(super) fn finish(ctx: &mut LowerCtx<'_>, deferred: bool, result: Operand) {
    if !std::mem::take(&mut ctx.binop.str_check.emitted) {
        return;
    }
    if deferred {
        ctx.binop.str_check.pending = true;
        return;
    }
    if !checks_here(ctx) {
        return;
    }
    let ty = ctx.operand_ty(&result);
    if ty.is_copy() {
        ctx.emit_throw_check(None);
    } else {
        ctx.emit_throw_check_owned(None, result, ty);
    }
}
