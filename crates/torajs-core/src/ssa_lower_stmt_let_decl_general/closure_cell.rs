//! Does a `let` initializer hand back the cell repr of a callable? The
//! answer [`super::initial_let_ty`] re-reprs a FnSig-annotated slot by.

use crate::ast::{Expr, ExprId};
use crate::ssa::Type;
use crate::ssa_lower::LowerCtx;

/// Rotations 358/359 — does this init expression hand back the CELL
/// repr of a callable? A FnSig-annotated slot fed a cell would
/// `call_indirect` the cell header (EXIT=138, the as-any hole's
/// family), so `initial_let_ty` re-reprs the slot to Closure when
/// this answers true. Keyed on SSA repr, never on the checked
/// Function type: a FnSig-repr source (forwarder-wrapped read bound
/// earlier) is a real code address and must keep direct dispatch.
///
/// - closure literal — every arrow lifts to `Expr::Closure` and
///   mints an env cell (chunk 734).
/// - ident — the source binding's recorded repr is Closure (r358).
/// - struct fn-field member read — the field slot is Closure-typed
///   by construction (`tag_struct_field_closure_types` retags every
///   struct-field fn annotation `__cls(`), so the read hands back a
///   cell (r358).
/// - call — the callee's ret repr is Closure (`effective_ret_ty`
///   upgrades a fn-sig-annotated ret whose body returns closures;
///   the value arriving here IS that cell) (r359).
/// - index — an element read of an array the checker typed with a
///   function element: the array's slots are Closure-repr.
/// - ternary / nullish — both arms yield cells, or one does and the
///   other is a null literal (r359; a mixed cell/code-address pair
///   keeps the FnSig repr and the pre-existing behavior — re-repring
///   the slot would just move which arm jumps wrong).
pub(super) fn init_yields_closure_cell(ctx: &LowerCtx, init: ExprId) -> bool {
    match ctx.ast.get_expr(init) {
        Expr::Closure { .. } => true,
        Expr::Ident(n) => {
            matches!(ctx.locals.get(n), Some(li) if matches!(li.ty, Type::Closure(_)))
        }
        Expr::Member { obj, name } => {
            let Some(crate::check::Type::Struct(fields)) = ctx.expr_types.get(obj) else {
                return false;
            };
            fields.iter().any(|(n, t)| {
                let t = match t {
                    crate::check::Type::Nullable(inner) => inner.as_ref(),
                    other => other,
                };
                n == name && matches!(t, crate::check::Type::Function(..))
            })
        }
        // an element of a function array, whatever the array expression
        // (`hs[0]`, `this.fs[0]`, `mk()[0]`, `hs.at(-1)!`): its slots are
        // Closure-repr (`ssa_lower_parse_type::parse_arr_suffix`), or
        // boxed when the element type admits `undefined`, a box that
        // carries the cell
        Expr::Index { .. } | Expr::As { .. } | Expr::Call { .. }
            if crate::check::face_slots::elem_source(ctx.ast, init).is_some_and(|obj| {
                matches!(ctx.expr_types.get(&obj),
                    Some(crate::check::Type::Array(elem))
                        if crate::check::face_slots::is_fn_elem(elem))
            }) =>
        {
            true
        }
        Expr::Call { callee, .. } => {
            matches!(ctx.ast.get_expr(*callee), Expr::Ident(n)
                if ctx.fn_table.get(n).is_some_and(
                    |fid| matches!(ctx.signatures.get(fid), Some(Type::Closure(_)))))
        }
        Expr::Ternary {
            then_branch,
            else_branch,
            ..
        } => cell_arms(ctx, [*then_branch, *else_branch]),
        Expr::Nullish { lhs, rhs } => cell_arms(ctx, [*lhs, *rhs]),
        _ => false,
    }
}

/// Both arms of a choice yield cells, or one does and the other is the
/// `null` / `undefined` literal (`d ? dbl : null` once `dbl` is wrapped):
/// a nullable cell.
fn cell_arms(ctx: &LowerCtx, arms: [ExprId; 2]) -> bool {
    let nullish = |e: ExprId| {
        matches!(ctx.ast.get_expr(e), Expr::Null)
            || matches!(ctx.ast.get_expr(e), Expr::Ident(n) if n == "undefined")
    };
    arms.iter()
        .all(|&a| nullish(a) || init_yields_closure_cell(ctx, a))
        && arms.iter().any(|&a| !nullish(a))
}
