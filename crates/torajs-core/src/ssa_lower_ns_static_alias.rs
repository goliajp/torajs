//! Reified namespace-static reads (`Array.from`, `Promise.resolve`) as
//! the receiver of `.call` / `.apply` / `.bind`.

use crate::ast::{Expr, ExprId};
use crate::ssa_lower::LowerCtx;

/// Whether `obj` reads a namespace static as a value, so `.call` /
/// `.apply` / `.bind` on it ride the any lane: the baked cell's runtime
/// dispatch honors the receiver channel (recv-first statics read the
/// thisArg as the constructor C; receiver-less ones ignore it per their
/// spec). Checker mirror: route_early's ns-static arm (RFC
/// 20260808-construct-channel B6 刀 2).
pub(crate) fn is_fn_read(ctx: &LowerCtx<'_>, obj: ExprId) -> bool {
    matches!(ctx.ast.get_expr(obj), Expr::Member { obj: ns, name: m }
        if matches!(ctx.ast.get_expr(*ns), Expr::Ident(n)
            if torajs_rc::ns_static::ns_static_id(n, m) >= 0))
}
