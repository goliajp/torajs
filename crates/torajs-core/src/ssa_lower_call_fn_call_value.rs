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
    // RFC 20260808-construct-channel B6 刀 2 — a reified
    // namespace-static read (`Array.from.call(C, …)` / `Math.max
    // .call(…)`) keeps the RUNTIME cell dispatch: the this-drop
    // replay below would eat the thisArg that recv-first statics
    // (Array.from / fromAsync) read as the constructor C. The
    // checker's ns-static `.call/.apply/.bind` arm types these Any,
    // so the any-method fallback lowers them; receiver-less statics
    // ignore the thisArg there per their spec, same answer as the
    // replay.
    if let Expr::Member { obj: ns, name: m } = ctx.ast.get_expr(obj)
        && let Expr::Ident(n) = ctx.ast.get_expr(*ns)
        && torajs_rc::ns_static::ns_static_id(n, m) >= 0
    {
        return None;
    }
    let rest: Vec<ExprId> = if is_call {
        args[1..].to_vec()
    } else {
        match args.len() {
            // §20.2.3.1 step 2 — an absent argArray means no args.
            1 => Vec::new(),
            2 => {
                let Expr::Array(els) = ctx.ast.get_expr(args[1]) else {
                    return None;
                };
                els.clone()
            }
            _ => return None,
        }
    };
    // Rotation 261 — a promoted fnexpr-this binding (`const f =
    // function () { …this… }` whose every use is a face read /
    // direct call / this replay) has the `__this: any` slot in its
    // native ABI; §20.2.3.3 / §20.2.3.1 bind `this` to the EXPLICIT
    // thisArg, so it boxes into that slot instead of dropping. The
    // closure_local this-entry owns the thisArg evaluation (ahead of
    // the rest args, spec order); a lane without the slot (variadic)
    // answers None and the call stays loud.
    if matches!(ctx.ast.get_expr(obj), Expr::Ident(n) if ctx.ast.fnexpr_recv_locals.contains(n)) {
        return crate::ssa_lower_call_closure_local::try_lower_with_this(
            ctx,
            eid,
            obj,
            &rest,
            Some(args[0]),
        );
    }
    // Rotation 261 — an INLINE promoted fn-expr callee
    // (`(function () { …this… }).apply(obj)`): lower the closure
    // (fresh minted env), box the thisArg into its leading `__this`
    // slot, release the env after the call consumed it.
    if matches!(ctx.ast.get_expr(obj), Expr::Closure { fn_name, .. }
        if ctx.ast.fnexpr_recv_fns.contains(fn_name))
    {
        let callee_op = ctx.lower_expr(obj);
        let crate::ssa::Type::Closure(sig) = ctx.operand_ty(&callee_op) else {
            return None;
        };
        let out = crate::ssa_lower_call_fn_indirect::emit_closure_callee_with_this(
            ctx,
            eid,
            callee_op,
            sig,
            &rest,
            crate::ssa_lower_call_fn_indirect::ClosureThis::Expr(args[0]),
        );
        ctx.release_owned_temp(obj, &callee_op);
        return Some(out);
    }
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
