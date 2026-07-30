//! `Iterator.from(O)` lowering per ES §27.1.6.2 — RFC
//! 20260730-iterator-global 刀 4.
//!
//! Statics wedge in the Map.groupBy shape: the single value boxes to
//! Any and dispatches to `__torajs_iterator_from` (torajs-anyvalue),
//! which runs GetIteratorFlattenable and answers the input itself
//! when it already sits on %Iterator.prototype%'s chain, else a
//! kind-WRAP IterHelper cell. Return surfaces as `Type::Any` — the
//! helper-cell method face is the runtime dispatcher's.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let Expr::Member { obj: ns_id, name } = ctx.ast.get_expr(callee) else {
        return None;
    };
    if name != "from" {
        return None;
    }
    let Expr::Ident(ns) = ctx.ast.get_expr(*ns_id) else {
        return None;
    };
    if ns != "Iterator" {
        return None;
    }
    if args.is_empty() {
        return None;
    }
    // value — box to Any (mirror the groupBy items arm; an Arr gets
    // its kind mark so the any lane reads slots correctly).
    let value_eid = args[0];
    let value_raw = ctx.lower_expr(value_eid);
    let value_ty = ctx.operand_ty(&value_raw);
    let (value_op, we_boxed) = match value_ty {
        Type::Any => (value_raw, false),
        Type::Arr(_) => {
            ctx.emit_arr_mark_kind(&value_raw);
            let as_i64 = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::PtrToInt(value_raw.clone()),
                Type::I64,
                None,
            );
            let as_any = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::IntToPtr(Operand::Value(as_i64)),
                Type::Any,
                None,
            );
            (Operand::Value(as_any), false)
        }
        _ => {
            if !ctx.expr_transfers_ownership(value_eid) && value_ty.is_refcounted() {
                ctx.emit_rc_inc(value_raw.clone());
            }
            (ctx.box_to_any(value_raw), true)
        }
    };
    // Trailing args — spec's from is unary; still lower for side
    // effects (groupBy precedent).
    for &a in args.iter().skip(1) {
        let _ = ctx.lower_expr(a);
    }
    let result = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.iterator_from, vec![value_op.clone()]),
        Type::Any,
        None,
    );
    // Kernel borrows the value (derives its own owned iterator).
    if we_boxed {
        ctx.emit_drop_value(value_op, Type::Any);
    } else {
        ctx.release_owned_temp(value_eid, &value_op);
    }
    ctx.emit_throw_check(None);
    Some(Operand::Value(result))
}
