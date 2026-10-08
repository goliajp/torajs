//! `recv.m(args)` where `recv` is typed by an interface that dispatches
//! through witness slots: the call goes through the receiver's vtable
//! row (`crate::ssa_lower_iface`), the same load-load-call a virtual
//! method takes. Arguments share like every other call lane: +0
//! borrows for bindings, owned temps released after the call.

use crate::ast::{Expr, ExprId};
use crate::ssa::{Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let Expr::Member { obj, name } = ctx.ast.get_expr(callee) else {
        return None;
    };
    let (obj, name) = (*obj, name.clone());
    if ctx.ast.iface_slots.is_empty()
        || !matches!(
            ctx.expr_types.get(&obj),
            Some(crate::check::Type::Struct(_))
        )
    {
        return None;
    }
    let recv_op = ctx.lower_expr(obj);
    let iface = match ctx.operand_ty(&recv_op) {
        Type::Obj(_) => crate::ssa_lower_iface::receiver_iface(ctx, obj),
        _ => None,
    };
    let base = ctx.ast.method_index.len();
    let Some(iface) = iface.filter(|i| ctx.ast.iface_slots.slot_of(base, i, &name).is_some())
    else {
        ctx.redispatch_lowered = Some((obj, recv_op));
        return None;
    };
    let mut argv = vec![recv_op];
    argv.extend(args.iter().map(|a| ctx.lower_expr(*a)));
    let owned_temps: Vec<(ExprId, Operand)> = args
        .iter()
        .zip(argv[1..].iter())
        .filter(|(a, _)| ctx.expr_owned_shape(**a))
        .map(|(a, op)| (*a, *op))
        .collect();
    let r = crate::ssa_lower_iface::emit_face_call(ctx, Some(eid), &iface, &name, argv, args)
        .unwrap_or_else(|| {
            panic!("ssa-lower: interface slot `{iface}.{name}` has no row signature")
        });
    for (a, op) in owned_temps {
        ctx.release_owned_temp(a, &op);
    }
    Some(r)
}
