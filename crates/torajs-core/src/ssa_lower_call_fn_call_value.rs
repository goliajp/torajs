//! L3b ⑥ — `Function.prototype.call` on a statically fn-typed VALUE
//! (`const f = add; f.call(u, 2, 3)`). The named-fn form was
//! rewritten away by the chunk-138 AST desugar and an any-held fn
//! stays on the runtime dispatch, so the only shape landing here is
//! a `FnSig` / `Closure`-repr value receiver. The thisArg lowers for
//! effect and drops (the desugar's no-this subset rule), then the
//! call replays the value-callee arms (`closure_local` /
//! `fn_indirect`) against the ORIGINAL call eid + the rest args —
//! the exact pair the checker's route_early arm forwarded to the
//! general admit, so the arity-pad table and per-arg records line up
//! by construction.

use crate::ast::{Expr, ExprId};
use crate::ssa::Operand;
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let Expr::Member { obj, name } = ctx.ast.get_expr(callee) else {
        return None;
    };
    if name != "call" || args.is_empty() {
        return None;
    }
    let obj = *obj;
    if !matches!(
        ctx.expr_types.get(&obj),
        Some(crate::check::Type::Function(..))
    ) {
        return None;
    }
    // thisArg evaluates for effect (§20.2.3.3 evaluates it), then
    // its fresh ownership ends here — the callee never sees it.
    let t_op = ctx.lower_expr(args[0]);
    let t_ty = ctx.operand_ty(&t_op);
    if t_ty.is_refcounted() && ctx.expr_is_fresh_owned(args[0]) {
        ctx.emit_drop_value(t_op, t_ty);
    }
    let rest = &args[1..];
    if let Some(op) = crate::ssa_lower_call_closure_local::try_lower(ctx, eid, obj, rest) {
        return Some(op);
    }
    crate::ssa_lower_call_fn_indirect::try_lower(ctx, eid, obj, rest)
}
