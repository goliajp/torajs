//! `Date.UTC(y, m?, d?, h?, min?, s?, ms?)` arity 1-6 trailing-
//! default padding pulled out of
//! [`crate::ssa_lower::lower_expr_inner`] `Expr::Call` dispatch as
//! chunk-40 of the `Expr::Call` god-arm decomp (chunks 1-39 = ... +
//! Symbol() callable ctor).
//!
//! S153 — `Date.UTC(y, m?, d?, h?, min?, s?, ms?)` per ES §21.4.2.21.
//! The runtime intrinsic `__torajs_date_utc_components` is statically
//! signed with 7 f64 args (spec ToNumber — NaN components invalidate); this arm pads trailing components with the
//! spec defaults (month=0, day=1, hour=0, min=0, sec=0, ms=0) when
//! the call site omits them. The 7-arg form falls through to the
//! generic resolve_callee path unchanged.
//!
//! Returns `Some(op)` on hit; `None` on miss (callee not
//! `Date.UTC(...)` Member-Ident, args empty, or arity already ≥ 7).

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let Expr::Member {
        obj: ns_id,
        name: m_name,
    } = ctx.ast.get_expr(callee)
    else {
        return None;
    };
    if m_name != "UTC" {
        return None;
    }
    let ns_id = *ns_id;
    let Expr::Ident(ns) = ctx.ast.get_expr(ns_id) else {
        return None;
    };
    if ns != "Date" || args.is_empty() || args.len() >= 7 {
        return None;
    }
    let mut arg_ops: Vec<Operand> = args
        .iter()
        .map(|a| {
            let v = ctx.lower_expr(*a);
            ctx.coerce_to_f64(v)
        })
        .collect();
    // defaults indexed by position (year=required so skipped;
    // month=0 day=1 hour=0 min=0 sec=0 ms=0)
    let defaults = [0.0f64, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0];
    while arg_ops.len() < 7 {
        arg_ops.push(Operand::ConstF64(defaults[arg_ops.len()]));
    }
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.date_utc_components, arg_ops),
        Type::F64,
        None,
    );
    Some(Operand::Value(v))
}
