//! `v(args…)` where the callee's static type is a KNOWN uncallable
//! value — `true()` / `f.length()` / `/re/()` / `undefined()`.
//!
//! §13.3.6.2 makes that a RUNTIME TypeError (step 6), reached after
//! the arguments evaluate (step 4), so the checker admits the site
//! (typed Any, see `check_type_of_call::general`) and this wedge
//! routes it through the runtime any-call dispatcher: the callee
//! boxes to an Any, argv packs like every any-call, and the
//! kernel's non-closure arm raises the catchable TypeError with the
//! right evaluation order for free. Keyed on the SAME
//! `Type::is_uncallable_value` predicate as the checker — single
//! source, no drift.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type as SsaType};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_any_method_call::pack_any_argv;

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    if !ctx
        .expr_types
        .get(&callee)
        .is_some_and(|t| t.is_uncallable_value())
    {
        return None;
    }
    let raw = ctx.lower_expr(callee);
    let raw_ty = ctx.operand_ty(&raw);
    // Borrow-shaped callees (an Ident binding, a Member read, the
    // fn-scope LICM-cached regex literal) don't own their value —
    // the box takes its own reference, exactly the argv discipline
    // in `pack_any_argv`, and asked the same way: the shared answer
    // reads through casts, which a roster of AST shapes cannot
    // (rotation 572). Regex stays this family's one difference.
    let is_borrow = !ctx.expr_is_fresh_owned(callee)
        || matches!(
            ctx.ast.get_expr(ctx.peel_as_wrappers(callee)),
            Expr::Regex { .. }
        );
    if is_borrow && raw_ty.is_refcounted() {
        ctx.emit_rc_inc(raw.clone());
    }
    let recv = ctx.box_to_any_from_expr(callee, raw);
    // Rotation 550 — the box is ours; park it across the arguments'
    // lowers so their throw paths release it.
    let recv_tok = ctx.push_throw_temp(recv.clone(), SsaType::Any);
    let packed = pack_any_argv(ctx, args);
    let argv = packed.argv;
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
        SsaType::Any,
        None,
    );
    packed.release(ctx);
    ctx.pop_throw_temp(recv_tok);
    // The boxed callee is ours (the borrow case took its own
    // reference above; a fresh temp moved its stake into the box).
    ctx.emit_drop_value(recv, SsaType::Any);
    ctx.emit_throw_check_owned(None, Operand::Value(result), SsaType::Any);
    Some(Operand::Value(result))
}
