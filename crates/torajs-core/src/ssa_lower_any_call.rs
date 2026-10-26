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

/// RFC 20260813-detached-objlit-method — the callee's REPRESENTATION
/// is `any` even though the checker named a sharper type.
///
/// A class method read off an instance (`const t = c.read`) types at
/// the method's signature: that is Phase I.1 in
/// `check_type_of_member`, and the type is not wrong — TypeScript
/// says the same. But a class method has no static function identity
/// to hand out, so the READ lowers to the runtime member-get and what
/// lands in the binding's slot is an Any (the runtime mints the
/// method value off the class-methods table; `typeof t` already
/// answers `function`). Dispatching on the checker type alone sent
/// the call looking for a FuncId by name — "unknown function `t`".
///
/// The slot is what the call has to work with, so it is what the
/// dispatch reads. Scope resolution comes from `ctx.locals`, so a
/// global fn shadowed by an any-typed local answers for the local,
/// as it should.
pub(crate) fn callee_slot_is_any(ctx: &LowerCtx<'_>, eid: crate::ast::ExprId) -> bool {
    matches!(ctx.ast.get_expr(eid), Expr::Ident(n)
        if ctx.locals.get(n).is_some_and(|i| i.ty == Type::Any))
}

/// Try to lower `callee(args…)` as a bare any-call. Returns `None`
/// unless the callee is a non-Member expression typing as `any`.
pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    eid: crate::ast::ExprId,
    callee: crate::ast::ExprId,
    args: &[crate::ast::ExprId],
) -> Option<Operand> {
    if matches!(ctx.ast.get_expr(callee), Expr::Member { .. }) {
        return None;
    }
    if !matches!(ctx.expr_types.get(&callee), Some(crate::check::Type::Any))
        && !callee_slot_is_any(ctx, callee)
    {
        return None;
    }
    Some(lower(ctx, eid, callee, args))
}

/// Lower `callee(args…)` through the runtime any-call dispatch,
/// whatever the callee's static type — the callee's VALUE is what
/// gets called. Also the lane for a call whose callee is an
/// unresolvable name (`ssa_lower_call::dispatch`): §6.2.5.5 GetValue
/// first — the global object's answer, or the recorded ReferenceError
/// — then §13.3.6.2 on whatever it answered. Returning the read alone
/// there handed `dyn(4)` the function itself when the object carried
/// `dyn` (rotation 639).
pub(crate) fn lower(
    ctx: &mut LowerCtx<'_>,
    eid: crate::ast::ExprId,
    callee: crate::ast::ExprId,
    args: &[crate::ast::ExprId],
) -> Operand {
    let recv = ctx.lower_expr(callee);
    emit(ctx, eid, callee, recv, args)
}

/// The call half of [`lower`], for a caller that already holds the
/// callee's value — a callee whose checker type is not `any` but
/// whose lowered value is (`(0, c.m)()`, `ssa_lower_call_fn_indirect`).
pub(crate) fn emit(
    ctx: &mut LowerCtx<'_>,
    eid: crate::ast::ExprId,
    callee: crate::ast::ExprId,
    recv: Operand,
    args: &[crate::ast::ExprId],
) -> Operand {
    if ctx.temps.tail.call == Some(eid) && !ctx.pins.operands.contains_key(&callee) {
        return crate::ssa_lower_tail_call::emit(ctx, eid, callee, recv, args);
    }
    // Rotation 550 — an owned callee temp (`f(1)(boom())`) is live
    // across the arguments' lowers; park it for their throw paths.
    let recv_tok = ctx.park_owned_temp(callee, &recv);
    let packed = pack_any_argv(ctx, args);
    let argc = crate::ast::source_argc(ctx.ast, eid, args) as i64;
    finish(ctx, callee, recv, recv_tok, packed, argc)
}

/// The call over a packed argv, and the releases after it.
pub(crate) fn finish(
    ctx: &mut LowerCtx<'_>,
    callee: crate::ast::ExprId,
    recv: Operand,
    recv_tok: Option<usize>,
    packed: crate::ssa_lower_any_argv::AnyArgv,
    argc: i64,
) -> Operand {
    let result = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.any_call,
            vec![recv, Operand::Value(packed.argv), Operand::ConstI64(argc)],
        ),
        Type::Any,
        None,
    );
    packed.release(ctx);
    ctx.unpark_owned_temp(recv_tok);
    // A Call-shaped callee (`f(1)(2)` — the inner call's owned Any
    // temp) is only borrowed by the runtime; release it here or the
    // curried hop leaks (mirrors the method-call arm's receiver
    // account). Borrow shapes (Ident) self-gate false.
    ctx.release_owned_temp(callee, &recv);
    // result is an OWNED Any already in hand — the throw path must
    // release it (mirrors the method-call arm; see
    // emit_throw_check_owned).
    ctx.emit_throw_check_owned(None, Operand::Value(result), Type::Any);
    Operand::Value(result)
}
