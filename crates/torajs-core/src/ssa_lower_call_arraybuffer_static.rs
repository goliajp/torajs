//! `ArrayBuffer.isView(arg)` lowering per §25.1.5.1 —
//! RFC 20260823-typedarray-substrate 刀 1.
//!
//! Statics wedge in the `Proxy.revocable` shape. The one argument
//! boxes to `any` because the answer is a question about its heap
//! tag, and only the any lane carries one.

use crate::ast::{Expr, ExprId};
use crate::ssa::{IPred, InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let Expr::Member { obj: ns_id, name } = ctx.ast.get_expr(callee) else {
        return None;
    };
    if name != "isView" {
        return None;
    }
    let Expr::Ident(ns) = ctx.ast.get_expr(*ns_id) else {
        return None;
    };
    if ns != "ArrayBuffer" {
        return None;
    }
    // The pair helper lowers two slots; `isView` reads only the
    // first, and the second is the `undefined` a missing argument
    // always is — which answers `false`, exactly as it should.
    let ops = crate::ssa_lower_new::lower_borrowed_any_pair(ctx, args);
    let arg = ops[0].0.clone();
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.arraybuffer_is_view, vec![arg]),
        Type::I64,
        None,
    );
    crate::ssa_lower_new::release_borrowed_any_pair(ctx, ops);
    // The kernel answers 0/1; the surface type is Boolean, so the
    // operand has to arrive as one.
    let b = ctx.f.append_inst(
        cur_block,
        InstKind::ICmp(IPred::Ne, Operand::Value(v), Operand::ConstI64(0)),
        Type::Bool,
        None,
    );
    Some(Operand::Value(b))
}
