//! `f(args…)` where the callee itself types as `any` — the bare
//! any-call arm (any-method-call RFC C4+, chunk 517).
//!
//! Sits right after the method-call arm in `ssa_lower_call`'s
//! cascade: Member callees stay with that arm (obj-typed Member
//! reads that answer `any` — getter-as-callee — remain a recorded
//! C4+ boundary), everything else whose static type erased to `any`
//! lowers to the runtime `__torajs_any_call` dispatch. The argv
//! ledger is the shared `pack_any_argv` (chunk-496 three-shape
//! rule); the runtime borrows argv and answers a caller-owned
//! AnyValue, non-closures raise a catchable TypeError the per-call
//! throw check propagates.

use crate::ast::Expr;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_any_method_call::pack_any_argv;

/// Try to lower `callee(args…)` as a bare any-call. Returns `None`
/// unless the callee is a non-Member expression typing as `any`.
pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: crate::ast::ExprId,
    args: &[crate::ast::ExprId],
) -> Option<Operand> {
    if matches!(ctx.ast.get_expr(callee), Expr::Member { .. }) {
        return None;
    }
    if !matches!(ctx.expr_types.get(&callee), Some(crate::check::Type::Any)) {
        return None;
    }
    let recv = ctx.lower_expr(callee);
    let (argv, boxed_slots) = pack_any_argv(ctx, args);
    let result = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.any_call,
            vec![
                recv.clone(),
                Operand::Value(argv),
                Operand::ConstI64(args.len() as i64),
            ],
        ),
        Type::Any,
        None,
    );
    for slot in boxed_slots.into_iter().flatten() {
        ctx.emit_drop_value(slot, Type::Any);
    }
    // A Call-shaped callee (`f(1)(2)` — the inner call's owned Any
    // temp) is only borrowed by the runtime; release it here or the
    // curried hop leaks (mirrors the method-call arm's receiver
    // account). Borrow shapes (Ident) self-gate false.
    ctx.release_owned_temp(callee, &recv);
    ctx.emit_throw_check(None);
    Some(Operand::Value(result))
}
