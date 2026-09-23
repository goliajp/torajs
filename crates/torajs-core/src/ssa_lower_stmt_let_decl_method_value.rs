//! Whether a let's initializer is a builtin METHOD VALUE read, and
//! which reified cell it names (`const p = parseInt`, `const m =
//! s.slice`).
//!
//! Split out of [`crate::ssa_lower_stmt_let_decl_general`] under the
//! 500-line hard limit; bodies verbatim. That file lowers a
//! declaration; these two answer one question about its initializer's
//! SHAPE, under the same three-part gate the member lowering mints
//! under, and each has a checker-side mirror that has to move with it.

use crate::ast::{Expr, ExprId};
use crate::ssa_lower::LowerCtx;

/// The table id when `init` is a namespace-static VALUE read
/// (`Math.max`) — the same three-part gate the member lowering mints
/// under: table hit + obj typed `Type::Object` (a user binding
/// shadowing `Math` never matches) + member typed `Function`.
pub(crate) fn ns_static_member_init_id(ctx: &LowerCtx, init: ExprId) -> Option<i64> {
    // rotation 368 — a bare global function property (`const p =
    // parseInt`) is the same cell through the ident lane; the alias
    // registers variadic so its calls take the boxed dual entry.
    if let Expr::Ident(gname) = ctx.ast.get_expr(init)
        && let Some(id) = crate::ssa_lower_ident::bare_global_ns_static_id(gname)
        && matches!(
            ctx.expr_types.get(&init),
            Some(crate::check::Type::Function(..))
        )
    {
        return Some(id);
    }
    let Expr::Member { obj, name } = ctx.ast.get_expr(init) else {
        return None;
    };
    let Expr::Ident(ns) = ctx.ast.get_expr(*obj) else {
        return None;
    };
    let id = torajs_rc::ns_static_id(ns, name);
    if id != torajs_rc::NS_STATIC_UNKNOWN
        && matches!(ctx.expr_types.get(obj), Some(crate::check::Type::Object(_)))
        && matches!(
            ctx.expr_types.get(&init),
            Some(crate::check::Type::Function(..))
        )
    {
        Some(id)
    } else {
        None
    }
}

/// The `(mid, proto family)` when `init` is a primitive-receiver
/// builtin method VALUE read (`s.slice` / `n.toString`) — the same
/// gate the member lowering mints under: the receiver types to a
/// prototype family + member typed `Function` + the name interns to
/// a mid with a spec meta row.
pub(crate) fn builtin_mv_member_init_mid(ctx: &LowerCtx, init: ExprId) -> Option<(i64, i64)> {
    let Expr::Member { obj, name } = ctx.ast.get_expr(init) else {
        return None;
    };
    let fam = crate::ssa_lower_member::mv_family_of_checker_ty(ctx.expr_types.get(obj)?)?;
    if !matches!(
        ctx.expr_types.get(&init),
        Some(crate::check::Type::Function(..))
    ) {
        return None;
    }
    let mid = torajs_rc::any_method_id(name);
    if mid == torajs_rc::ANY_METHOD_UNKNOWN || torajs_rc::any_method_meta(mid).is_none() {
        return None;
    }
    Some((mid, fam))
}
