//! Reified namespace-static reads (`Array.from`, `Promise.resolve`) and
//! the bindings that hold one (`const r = Promise.resolve`).

use crate::ast::{Expr, ExprId};
use crate::ssa::ValueId;
use crate::ssa_lower::LowerCtx;

impl LowerCtx<'_> {
    /// What a `let` of `name` just lowered leaves for later reads: its
    /// annotation, and its namespace-static alias.
    pub(crate) fn note_let(
        &mut self,
        name: &str,
        outer: Option<ValueId>,
        ann: Option<&String>,
        init: ExprId,
    ) {
        self.note_binding_ann(name, outer, ann);
        note_binding(self, name, outer, init);
    }
}

/// Register the binding `name` a `let` just lowered when its init reads
/// a namespace static. Keyed by the binding's own slot, so an inner
/// binding that shadows it carries no entry and the outer one's comes
/// back with its `locals` entry when the block closes; `outer` is the
/// slot the name denoted before (a lane that kept the binding out of
/// `locals` leaves it unchanged, and registers nothing).
fn note_binding(ctx: &mut LowerCtx<'_>, name: &str, outer: Option<ValueId>, init: ExprId) {
    let Some(slot) = ctx.locals.get(name).map(|l| l.slot) else {
        return;
    };
    if outer == Some(slot) {
        return;
    }
    if let Some(id) =
        crate::ssa_lower_stmt_let_decl_method_value::ns_static_member_init_id(ctx, init)
    {
        let member = matches!(ctx.ast.get_expr(init), Expr::Member { .. });
        ctx.ns_static_locals.insert(slot, (id, member));
    }
}

/// The (table id, spelled `Ns.m`) of the binding `name` denotes here.
/// The flag is the checker's `builtin_mv` face; a bare-global alias
/// (`const p = parseInt`) is registered but is not that face.
pub(crate) fn of_ident(ctx: &LowerCtx<'_>, name: &str) -> Option<(i64, bool)> {
    ctx.ns_static_locals
        .get(&ctx.locals.get(name)?.slot)
        .copied()
}

/// Whether `obj` reads a namespace static as a value, so `.call` /
/// `.apply` / `.bind` on it ride the any lane: the baked cell's runtime
/// dispatch honors the receiver channel (recv-first statics read the
/// thisArg as the constructor C; receiver-less ones ignore it per their
/// spec). Checker mirror: route_early's ns-static arm (RFC
/// 20260808-construct-channel B6 刀 2). A binding holding such a read
/// (`const r = Promise.resolve; r.call(Promise, 7)`) is the same cell
/// under another name, and the checker's `builtin_mv` binding arm
/// admits it as Any too.
pub(crate) fn is_fn_read(ctx: &LowerCtx<'_>, obj: ExprId) -> bool {
    match ctx.ast.get_expr(obj) {
        Expr::Member { obj: ns, name: m } => matches!(ctx.ast.get_expr(*ns),
            Expr::Ident(n) if torajs_rc::ns_static::ns_static_id(n, m) >= 0),
        Expr::Ident(n) => of_ident(ctx, n).is_some_and(|(_, member)| member),
        _ => false,
    }
}
