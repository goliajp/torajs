//! L3b ⑥ — `Function.prototype.call` / `.apply` on a statically
//! fn-typed VALUE (`const f = add; f.call(u, 2, 3)` /
//! `f.apply(u, [2, 3])`). The named-fn form was rewritten away by
//! the chunk-138 AST desugar and an any-held fn stays on the runtime
//! dispatch, so the only shape landing here is a `FnSig` /
//! `Closure`-repr value receiver. The thisArg lowers for effect and
//! drops (the desugar's no-this subset rule), then the call replays
//! the value-callee arms (`closure_local` / `fn_indirect`) against
//! the ORIGINAL call eid + the rest args — the exact pair the
//! checker's route_early arm forwarded to the general admit, so the
//! arity-pad table and per-arg records line up by construction.
//! `apply` takes the LITERAL argsArray form only (the desugar's own
//! bound; the checker gate is the same, so a runtime array never
//! reaches here).

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
    if (name != "call" && name != "apply") || args.is_empty() {
        return None;
    }
    let is_call = name == "call";
    let obj = *obj;
    if !matches!(
        ctx.expr_types.get(&obj),
        Some(crate::check::Type::Function(..))
    ) {
        return None;
    }
    let rest: Vec<ExprId> = if is_call {
        args[1..].to_vec()
    } else {
        if args.len() != 2 {
            return None;
        }
        let Expr::Array(els) = ctx.ast.get_expr(args[1]) else {
            return None;
        };
        els.clone()
    };
    // thisArg evaluates for effect (§20.2.3.3 evaluates it), then
    // its fresh ownership ends here — the callee never sees it.
    let t_op = ctx.lower_expr(args[0]);
    let t_ty = ctx.operand_ty(&t_op);
    if t_ty.is_refcounted() && ctx.expr_is_fresh_owned(args[0]) {
        ctx.emit_drop_value(t_op, t_ty);
    }
    if let Some(op) = crate::ssa_lower_call_closure_local::try_lower(ctx, eid, obj, &rest) {
        return Some(op);
    }
    if let Some(op) = crate::ssa_lower_call_fn_indirect::try_lower(ctx, eid, obj, &rest) {
        return Some(op);
    }
    // Direct member form (`Math.max.call(...)` — the value was never
    // bound to a local): neither replay arm admits a Member obj, but
    // the member VALUE read itself lowers (the ns-static mint /
    // fn-typed field arms), so lower it and dispatch on the SSA
    // repr. An ns-static cell must take the BOXED dual entry — its
    // fn_addr is the typed-slot boundary throw, the real dispatcher
    // lives behind CLOSURE_BOXED_ENTRY_OFF (the same lane
    // variadic_locals routes an alias call through); every other
    // Closure/FnSig repr keeps the env-first / direct emitters the
    // generalized-indirect arm uses. A non-callable repr falls
    // through to the resolve_callee panic exactly as before.
    if matches!(ctx.ast.get_expr(obj), Expr::Member { .. }) {
        let is_ns_static =
            crate::ssa_lower_stmt_let_decl_general::ns_static_member_init_id(ctx, obj).is_some();
        let callee_op = ctx.lower_expr(obj);
        match ctx.operand_ty(&callee_op) {
            crate::ssa::Type::Closure(sig) if is_ns_static => {
                return Some(
                    crate::ssa_lower_call_closure_local::emit_variadic_boxed_call(
                        ctx, callee_op, sig, &rest,
                    ),
                );
            }
            crate::ssa::Type::Closure(sig) => {
                return Some(crate::ssa_lower_call_fn_indirect::emit_closure_callee(
                    ctx, eid, callee_op, sig, &rest,
                ));
            }
            crate::ssa::Type::FnSig(sig) => {
                return Some(crate::ssa_lower_call_fn_indirect::emit_fnsig_callee(
                    ctx, eid, callee_op, sig, &rest,
                ));
            }
            _ => {}
        }
    }
    None
}
