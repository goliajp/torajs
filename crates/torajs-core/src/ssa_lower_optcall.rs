//! `Expr::OptCall { callee, args }` lowering — `callee?.(args…)`
//! optional call (ES2020 §13.3.9, chunk 705). The OptChain sibling
//! owns `?.field`, OptIndex owns `?.[index]`; this file owns the
//! call shape.
//!
//! Same contract as the siblings: the callee evaluates exactly once;
//! a nullish callee short-circuits to an ANY_UNDEF box WITHOUT
//! evaluating the arguments (they lower inside the hit block); a
//! non-nullish non-callable callee is the runtime `__torajs_any_call`
//! catchable TypeError. Four callee routes, keyed off the checker
//! type (mirrors `check_type_of_misc::check_opt_call`):
//!
//! - **plain** (statically non-nullish) — `?.()` ≡ `()`: delegate to
//!   the regular Call dispatcher verbatim (`g?.(21)` on a closure
//!   binding rides the typed fast path, result keeps its concrete
//!   type).
//! - **Null / Undefined** — statically nullish: the callee lowers
//!   for side effects, the args never lower, the result is a
//!   constant ANY_UNDEF box.
//! - **Any** — NaN-box tag guard (shared `emit_nullish_cond`); the
//!   hit path packs the argv (`pack_any_argv` three-shape ledger)
//!   and dispatches `__torajs_any_call`. A Member callee on an any
//!   receiver reaches this route through the normal member-read
//!   lowering, so `o.m?.(1)` probes the property once — dynobj
//!   closure fields invoke, an absent field answers undefined.
//!   Builtin-method reification (`s.toUpperCase?.()` on an any
//!   string) inherits the chunk-521 member-read boundary (the read
//!   answers undefined on non-dynobj receivers) — recorded face.
//! - **Nullable** — raw ptr null guard; the hit path boxes the
//!   unwrapped value (borrow inc / owned transfer per the pack
//!   ledger) and dispatches `__torajs_any_call` (a non-closure
//!   inner, e.g. `Arr<Str> | null`, is its catchable TypeError).

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Terminator, Type};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_any_method_call::pack_any_argv;
use crate::ssa_lower_optindex::emit_nullish_cond;

pub(crate) fn lower(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    callee: ExprId,
    args: &[ExprId],
) -> Operand {
    match ctx.expr_types.get(&callee) {
        Some(crate::check::Type::Null) | Some(crate::check::Type::Undefined) => {
            let v = ctx.lower_expr(callee);
            ctx.release_owned_temp(callee, &v);
            emit_undef_box(ctx)
        }
        Some(crate::check::Type::Any) | Some(crate::check::Type::Nullable(_)) => {
            lower_guarded(ctx, callee, args)
        }
        _ => crate::ssa_lower_call::lower(ctx, eid, callee, args),
    }
}

fn lower_guarded(ctx: &mut LowerCtx<'_>, callee: ExprId, args: &[ExprId]) -> Operand {
    let transfers = ctx.expr_transfers_ownership(callee);
    let v = ctx.lower_expr(callee);
    let v_ty = ctx.operand_ty(&v);
    let res_slot = ctx.alloca_in_entry(Type::Any, Some("__optcall"));
    let nullish = emit_nullish_cond(ctx, &v, &v_ty);
    let null_blk = ctx.f.add_block();
    let hit_blk = ctx.f.add_block();
    let after = ctx.f.add_block();
    let cb = ctx.cur_block;
    ctx.f.set_term(
        cb,
        Terminator::CondBr {
            cond: nullish,
            then_blk: null_blk,
            else_blk: hit_blk,
        },
    );
    // Miss path → ANY_UNDEF box. A ptr-typed owned callee temp
    // (Nullable fn-call result) was never consumed here — release
    // its ref (dead when v is statically non-null).
    ctx.cur_block = null_blk;
    let undef_box = emit_undef_box(ctx);
    ctx.f.append_void(
        null_blk,
        InstKind::Store(undef_box, Operand::Value(res_slot), 0),
    );
    if !matches!(v_ty, Type::Any) && transfers && v_ty.is_refcounted() {
        ctx.emit_drop_value(v.clone(), v_ty.clone());
    }
    ctx.f.set_term(null_blk, Terminator::Br(after));
    // Hit path — the arguments lower HERE (short-circuit never
    // evaluates them).
    ctx.cur_block = hit_blk;
    let (callee_av, we_boxed) = if matches!(v_ty, Type::Any) {
        (v.clone(), false)
    } else {
        // pack ledger: box_to_any is TRANSFER — a borrowed value
        // rc-incs first, an owned temp hands its ref to the box.
        if !transfers && v_ty.is_refcounted() {
            ctx.emit_rc_inc(v.clone());
        }
        (ctx.box_to_any(v.clone()), true)
    };
    let (argv, boxed_slots) = pack_any_argv(ctx, args);
    let result = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.any_call,
            vec![
                callee_av.clone(),
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
    if we_boxed {
        ctx.emit_drop_value(callee_av, Type::Any);
    }
    ctx.emit_throw_check(None);
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(result), Operand::Value(res_slot), 0),
    );
    let hb = ctx.cur_block;
    ctx.f.set_term(hb, Terminator::Br(after));
    ctx.cur_block = after;
    // An Any-typed owned callee temp (member read / chained call)
    // was borrowed by the dispatch on the hit path and untouched on
    // the miss path — one release covers both.
    if matches!(v_ty, Type::Any) {
        ctx.release_owned_temp(callee, &v);
    }
    let r = ctx.f.append_inst(
        after,
        InstKind::Load(Type::Any, Operand::Value(res_slot), 0),
        Type::Any,
        None,
    );
    Operand::Value(r)
}

/// Constant `undefined` result box (spec: the short-circuit value is
/// undefined, not null).
fn emit_undef_box(ctx: &mut LowerCtx<'_>) -> Operand {
    let b = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.any_box,
            vec![Operand::ConstI64(5), Operand::ConstI64(0)],
        ),
        Type::Any,
        None,
    );
    Operand::Value(b)
}
