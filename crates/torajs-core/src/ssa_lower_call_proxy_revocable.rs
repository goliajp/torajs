//! `Proxy.revocable(target, handler)` lowering per §28.2.2.1 —
//! RFC 20260823-proxy-substrate 刀 3.
//!
//! Statics wedge in the `Iterator.from` shape. Both arguments box to
//! `any` — and a literal argument takes the dynobj lane for the same
//! reason `new Proxy` does (`ssa_lower_new::lower_proxy`): a proxy
//! reaches its target and its handler only through the any lane.

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
    if name != "revocable" {
        return None;
    }
    let Expr::Ident(ns) = ctx.ast.get_expr(*ns_id) else {
        return None;
    };
    if ns != "Proxy" {
        return None;
    }
    let ops = crate::ssa_lower_new::lower_proxy_args(ctx, args);
    let argv: Vec<Operand> = ops.iter().map(|(op, _, _)| op.clone()).collect();
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.proxy_revocable, argv),
        Type::Any,
        None,
    );
    crate::ssa_lower_new::release_proxy_args(ctx, ops);
    ctx.emit_throw_check(None);
    Some(Operand::Value(v))
}
