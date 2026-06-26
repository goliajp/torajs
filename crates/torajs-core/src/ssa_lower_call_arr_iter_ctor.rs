//! Array iterator ctor dispatch (`xs.keys()` / `xs.values()` /
//! `xs.entries()`) pulled out of
//! [`crate::ssa_lower::lower_expr_inner`] `Expr::Call` dispatch as
//! chunk-36 of the `Expr::Call` god-arm decomp (chunks 1-35 = ... +
//! RegExp instance method dispatch).
//!
//! v0.2 #1 P6.4c-C3 — `xs.{keys|values|entries}()` on `Type::Arr<Any>`
//! receivers. Routes through the `__torajs_arr_iter_create_*`
//! intrinsics returning `Type::ArrIter` (parallel to Map's MapIter).
//! Restricted to Array<Any> by check.rs; typed-T arrays would have an
//! 8B-per-slot layout the runtime can't walk uniformly (P5.4
//! follow-up).
//!
//! S292 — keys / values / entries are 0-arg per spec but accept any
//! trailing operands per ES trailing-arg ignore; we lower-and-drop
//! them before the 1-arg helper Call (S272 idiom).
//!
//! Returns `Some(op)` on hit; `None` on miss (callee not a Member-
//! call, method name not in {keys, values, entries}, or receiver isn't
//! `Type::Arr(_)`). Receiver is lowered idempotently before the
//! type-gate so a non-Array receiver (Map / Set / class instance with
//! the same method names) falls through to its own arm without
//! triggering side-effect duplication on re-lower.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let Expr::Member { obj, name } = ctx.ast.get_expr(callee) else {
        return None;
    };
    if !matches!(name.as_str(), "keys" | "values" | "entries") {
        return None;
    }
    let recv_id = *obj;
    let method = name.clone();
    let recv_op = ctx.lower_expr(recv_id);
    let recv_ty = ctx.operand_ty(&recv_op);
    if !matches!(recv_ty, Type::Arr(_)) {
        // Fall through — recv_op already lowered; the downstream
        // Map/Set/class-instance arm re-lowers the receiver and the
        // load is idempotent (no side-effect duplication).
        return None;
    }
    // S292 — trailing operands lower-and-drop per ES trailing-arg
    // ignore. spec arity is 0; we lower them for the side effect.
    for &a in args.iter() {
        let _ = ctx.lower_expr(a);
    }
    let target = match method.as_str() {
        "keys" => ctx.intrinsics.arr_iter_create_keys,
        "values" => ctx.intrinsics.arr_iter_create_values,
        "entries" => ctx.intrinsics.arr_iter_create_entries,
        _ => unreachable!(),
    };
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(target, vec![recv_op]),
        Type::ArrIter,
        None,
    );
    Some(Operand::Value(v))
}
