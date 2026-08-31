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
    // `.values()` / `.entries()` read each slot through the ArrIter
    // step's kind-aware `__torajs_arr_get_any_boxed`, which reboxes a
    // typed (8B-slot) Array<T> per its recorded elem kind. A typed
    // literal is ARR_KIND_UNSET until a coercion site marks it, so this
    // Arr<T> → Arr<Any> read site marks it here (idempotent; a no-op on
    // Array<Any> and on already-marked blocks).
    //
    // Rotation 543 — `.keys()` used to be excluded, on the true
    // observation that it yields indices and never reads a slot. But
    // the STEP is not the only consumer of the mark: the iterator
    // holds the last strong ref to the source in
    // `const t = xs.keys()`, so the array dies inside
    // `__torajs_arr_iter_drop` -> `__torajs_value_drop_heap`, and
    // THAT reads the recorded elem kind to decide whether the slots
    // hold cells. Unmarked, it dropped the block and left every
    // element behind: `const zs = ["a" + i]; zs.keys();` peaked at
    // 8.00 MB RSS over 200k churn against 1.61 MB for the same loop
    // over `[1, 2]`, whose slots have nothing to drop.
    ctx.emit_arr_mark_kind(&recv_op);
    let target = match method.as_str() {
        "keys" => ctx.intrinsics.arr_iter_create_keys,
        "values" => ctx.intrinsics.arr_iter_create_values,
        "entries" => ctx.intrinsics.arr_iter_create_entries,
        _ => unreachable!(),
    };
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(target, vec![recv_op.clone()]),
        Type::ArrIter,
        None,
    );
    // The create helper takes its own +1 on the source array (iter
    // holds a strong ref), so an owned-shape receiver (an array
    // literal built for this call) still holds its stranded stake
    // here: release it (chunk 744 fromEntries idiom; churn 200k
    // `[1,2].values()` leaked the 72B arr cell every iteration —
    // rc parked at 1, scan_black'd out of the cycle buffer, 32.2MB
    // vs 6.4MB flat). Ident-read receivers are borrows — no-op
    // through the predicate.
    ctx.release_owned_temp(recv_id, &recv_op);
    Some(Operand::Value(v))
}
