//! `Expr::Assign` lowering — the target-shape dispatch moved out of
//! [`crate::ssa_lower_expr_inner`] (rotation 655, file-size: that
//! dispatch table sits at its cap), plus the per-expression operand
//! pins the assignment forms share with their embedded reads.

use crate::ast::{Expr, ExprId};
use crate::ssa::Operand;
use crate::ssa_lower::LowerCtx;

/// Operands one assignment expression parks for reuse by a second
/// site inside the same expression (see the field docs; every entry
/// is set and cleared by the lowering that owns the expression).
#[derive(Default)]
pub(crate) struct SharedPins {
    /// Cluster #4 T4 — compound `o[k] op= v` coerced-key pin: the keyed
    /// write lane runs ToPropertyKey once and parks (key eid, coerced key)
    /// here so the embedded read reuses it (§6.2.5 GetValue writes the
    /// coerced key back into the Reference Record — one coerce total).
    pub(crate) compound_key: Option<(ExprId, Operand)>,
}

/// `Expr::Assign` target dispatch — Ident / Member / Index shapes
/// route to their per-shape sibling lowerers.
pub(crate) fn lower(ctx: &mut LowerCtx, eid: ExprId, target: ExprId, value: ExprId) -> Operand {
    match ctx.ast.get_expr(target).clone() {
        Expr::Ident(name) => {
            // K.3 module-level data global + local-binding
            // assign (4-layer coercion: F64←I64 / Any←val
            // box_to_any / num←Any coerce / Str←Any
            // coerce_to_str), plus the RFC 20260730 undeclared-
            // write ReferenceError lane keyed by target eid. See
            // [`crate::ssa_lower_assign_ident::lower`].
            crate::ssa_lower_assign_ident::lower(ctx, eid, target, name, value)
        }
        Expr::Member { obj, name: field } => {
            // M1.4 — `obj.field = value`. 7-way dispatch
            // (Type::Any dynobj / Closure props / FnSig
            // fnprops / Arr length setter / Arr arrprops /
            // RegExp lastIndex / struct field store with
            // setter accessor + frozen guard). See
            // [`crate::ssa_lower_assign_member::lower`].
            crate::ssa_lower_assign_member::lower(ctx, eid, obj, field, value)
        }
        Expr::Index { obj, index } => {
            // bug-327 C3 — moved to ssa_lower_index_assign.rs
            // (bounds-honoring write: Array<Any> grows via
            // __torajs_arr_set_any_grow + write-back, typed
            // tier guards the inline store).
            ctx.lower_index_assign(eid, obj, index, value)
        }
        other => panic!("ssa-lower: unsupported assign target: {other:?}"),
    }
}
