//! `recv.m(args)` where `recv` is typed by an interface that dispatches
//! through witness slots: the call goes through the receiver's vtable
//! row (`crate::ssa_lower_iface`), the same load-load-call a virtual
//! method takes. Arguments share like every other call lane: +0
//! borrows for bindings, owned temps released after the call.
//!
//! A receiver the checker types by an object type may still be held
//! as `any`: an element of a `(T | undefined)[]` literal, which lives
//! on the any lane, read back or bound by a `for-of`. That
//! representation says nothing about the layout behind it, so its
//! method is looked up by name on the value itself (§13.3.6.2, the
//! receiver as `this`), as for any other `any` receiver.

use crate::ast::{Expr, ExprId};
use crate::check::Type as T;
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
    let object_recv = match ctx.expr_types.get(&obj) {
        Some(T::Struct(_)) => true,
        Some(T::Nullable(t)) => matches!(**t, T::Struct(_)),
        _ => false,
    };
    if !object_recv
        && (ctx.ast.iface_slots.is_empty()
            || !matches!(ctx.expr_types.get(&obj), Some(T::Nullable(_))))
    {
        return None;
    }
    let recv_op = ctx.lower_expr(obj);
    if ctx.operand_ty(&recv_op) == Type::Any {
        return Some(any_recv_call(ctx, eid, obj, recv_op, &name, args));
    }
    if ctx.ast.iface_slots.is_empty() {
        ctx.redispatch_lowered = Some((obj, recv_op));
        return None;
    }
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

/// The method call on a receiver held as `any`, through the any lane's
/// dispatcher; its answer is an owned `any`.
fn any_recv_call(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    obj: ExprId,
    recv: Operand,
    name: &str,
    args: &[ExprId],
) -> Operand {
    let passed = &args[..crate::ast::source_argc(ctx.ast, eid, args)];
    let parked = ctx.park_owned_temp(obj, &recv);
    let r = crate::ssa_lower_any_method_call::emit_any_method_call(
        ctx,
        recv,
        name,
        Operand::ConstPtrNull,
        passed,
    );
    ctx.unpark_owned_temp(parked);
    ctx.release_owned_temp(obj, &recv);
    ctx.emit_throw_check_owned(None, Operand::Value(r), Type::Any);
    Operand::Value(r)
}
