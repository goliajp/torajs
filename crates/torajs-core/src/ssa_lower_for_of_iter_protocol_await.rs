//! The async half of the iterator-protocol for-of lane
//! ([`crate::ssa_lower_for_of_iter_protocol`]).
//!
//! ES §14.7.5.10 — `for await (const v of it)` awaits the result of
//! every `next()` before reading `done` / `value` off it. An async
//! generator's step methods answer `Promise<IteratorResult>` per
//! §27.6 (the class-method async rewrite collapses their declared
//! return type to `Promise<__step_*>`), so the sync lane's direct
//! field loads would be reading a Promise header as a struct.
//!
//! Split out rather than inlined: the parent sits at 461 lines
//! against the 500 hard limit, and these two pieces are cohesive —
//! "how the async form differs" in one place.

use crate::ast::Stmt;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_parse_type::parse_type;

/// The IteratorResult struct hiding inside `next()`'s declared
/// `Promise<T>` return type.
///
/// SSA's `Type::Promise` is type-erased (one 32-byte heap shape for
/// every T), so the resolved signature cannot answer this — the
/// declared annotation on the FnDecl is where T survives. Mirrors
/// what `ssa_lower_member_promise_value` does for `p.value`, which
/// recovers T from the checker's `Type::Promise(inner)` verdict.
pub(crate) fn awaited_step_ty(ctx: &mut LowerCtx<'_>, next_fn: &str) -> Option<Type> {
    let declared = ctx.ast.stmts.iter().find_map(|s| match s {
        Stmt::FnDecl {
            name, return_type, ..
        } if name == next_fn => return_type.clone(),
        _ => None,
    })?;
    let inner = declared.strip_prefix("Promise<")?.strip_suffix('>')?;
    let parsed = parse_type(
        Some(inner),
        ctx.aliases,
        ctx.arr_layouts,
        ctx.fn_sigs,
        ctx.generic_struct_decls,
        ctx.struct_layouts,
        ctx.inst_memo,
    );
    // The sync lane reaches its step struct as the step method's
    // declared return type, so it takes the width injection every
    // annotation-consuming site takes. This one arrives wrapped in
    // `Promise<...>`, which is type-erased, so `widen_container_ty`
    // saw a `Type::Promise` and passed it through — the read came back
    // as the parse-default `value: I64` while the literal that built
    // the step had widened, and an integer yield read back as its own
    // f64 bit pattern (552-04: a `try`/`finally` generator ANYWHERE in
    // the program floats the shared `{value, done}` class). The key is
    // the step method's own `Ret`, the same one the sync lane uses.
    Some(crate::ssa_lower_container_width::widen_container_ty(
        parsed,
        Some(inner),
        &crate::num_width::SlotKey::Ret(next_fn.to_string()),
        ctx.num_f64_slots,
        ctx.arr_layouts,
        ctx.struct_layouts,
        ctx.fn_sigs,
    ))
}

/// One await of a step Promise, answering the IteratorResult it
/// settled to.
///
/// Same four beats `await e` lowers to (`ssa_lower_member_promise_value`):
/// drain the microtask queue so the generator body actually runs,
/// read the settled payload, check for a rejection, then adopt the
/// payload — the raw i64 is a borrowed view into the Promise, so it
/// takes an rc of its own before the Promise it came out of is
/// released. `next()` hands us a fresh Promise every step; nothing
/// else holds it, so it is dropped here rather than leaked per-iter.
pub(crate) fn emit_await_step(
    ctx: &mut LowerCtx<'_>,
    promise_val: crate::ssa::ValueId,
    step_ret_ty: Type,
) -> crate::ssa::ValueId {
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.microtask_drain, vec![]),
    );
    let raw = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.promise_get_value,
            vec![Operand::Value(promise_val)],
        ),
        Type::I64,
        None,
    );
    // A rejected step is an abrupt completion of the loop (§7.4.6),
    // and what a rejected promise answers is a sentinel, not a
    // struct — checking before the IntToPtr keeps the field loads
    // below off a wild pointer.
    ctx.emit_throw_check(None);
    let step = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::IntToPtr(Operand::Value(raw)),
        step_ret_ty,
        None,
    );
    ctx.emit_rc_inc(Operand::Value(step));
    ctx.emit_drop_value(Operand::Value(promise_val), Type::Promise);
    step
}
