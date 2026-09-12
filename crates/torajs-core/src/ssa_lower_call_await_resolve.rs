//! RFC 20260805-async-fn-state-machine blade 1 — `__torajs_await_resolve(v)`.
//!
//! §27.7.5.3 Await step 2 is `PromiseResolve(%Promise%, v)`: the
//! INTRINSIC operation, reached without reading any property. The
//! driver the async desugar injects needs exactly that, and could not
//! spell it: `Promise.resolve(v)` in source honours a user patch or
//! accessor on the static (§13.3.6.1 — the lowering probes it first),
//! so a driver written that way counted an extra get + call on every
//! await (`promise-resolve-accessor-get-once-001`) and ran the user's
//! replacement where the spec runs the intrinsic
//! (`promise-static-patch-*`). This synthetic call is the spelling the
//! driver uses. The checker types it through the bare name's entry in
//! `check_type_of_ident_synth` (`(any) => Promise<any>`); the general
//! call path does the rest, and this arm owns the lowering.
//!
//! Straight to the any-lane kernel `promise_resolve_any`, with no
//! patch probe in front of it: the kernel answers the argument itself
//! when it is a %Promise% cell (§27.2.4.7 step 2, adopting the
//! transferred ref) and otherwise a fresh cell resolved with it — a
//! thenable's `then` is read once, per §27.2.1.3.2. That read can run
//! user code, so the throw check follows the call. Borrow-shaped
//! arguments share (+1) exactly as the `Promise.resolve` any lane does.
//!
//! Only an `any` argument is lowered here — the injected driver passes
//! `r.value` off an `any` step object, and that is the one caller. A
//! typed argument falls through to the plain-call lanes and their
//! honest "unknown function" reject.

use crate::ast::{Expr, ExprId};

pub(crate) const NAME: &str = "__torajs_await_resolve";
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let Expr::Ident(n) = ctx.ast.get_expr(callee) else {
        return None;
    };
    if n != NAME || args.len() != 1 {
        return None;
    }
    let arg0 = args[0];
    let arg_op = ctx.lower_expr(arg0);
    let arg_ty = ctx.operand_ty(&arg_op);
    if !matches!(arg_ty, Type::Any) {
        return None;
    }
    if !ctx.expr_transfers_ownership(arg0) {
        ctx.emit_owned_result_inc(arg_op.clone(), arg_ty);
    }
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.promise_resolve_any, vec![arg_op]),
        Type::Promise,
        None,
    );
    ctx.emit_throw_check(None);
    Some(Operand::Value(v))
}
