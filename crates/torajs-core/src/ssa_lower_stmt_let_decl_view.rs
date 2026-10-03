//! The binding lane's side of the alias-class census
//! (`ast::let_owned_elems_alias`): does this declaration's class hold
//! a split product as views?
//!
//! The census connects the bindings that only ever hand a product to
//! each other and admits a class on shape alone — it runs before the
//! checker and cannot see that `x.split(",")` on an `any` receiver,
//! or with an `any` separator, answers something other than an
//! `Arr<Substr>`. The lane asks that here, with the checker's types,
//! for every split in the class at once, so every member reaches the
//! same answer: views for all (the `[]` and alias inits take the
//! `Arr<Substr>` layout at declaration), or the class is unfit and
//! every member holds owned elements, exactly as a class the census
//! poisoned does.

use crate::ast::{Expr, ExprId};
use crate::ssa::Type;
use crate::ssa_lower::LowerCtx;

/// `None` — the init is in no view class; `Some(fit)` — whether the
/// class's every split call lowers through the view kernel.
pub(crate) fn view_slot_verdict(ctx: &LowerCtx<'_>, init: ExprId) -> Option<bool> {
    let members = ctx.ast.view_slot_inits.get(&init)?;
    Some(members.iter().all(|m| match ctx.ast.get_expr(*m) {
        Expr::Call { .. } => crate::ssa_lower_toplevel_globals::init_is_static_string_split(
            *m,
            ctx.ast,
            ctx.expr_types,
        ),
        _ => true,
    }))
}

/// The slot type a view-class member declares: an owned-string array
/// annotation becomes the view layout the class shares, and a
/// container of them (`ast::let_owned_elems_nest`) holds that layout
/// in every element.
pub(crate) fn view_slot_ty(ctx: &mut LowerCtx<'_>, ty: Type) -> Type {
    let Type::Arr(aid) = ty else { return ty };
    match ctx.arr_layouts[aid.0 as usize] {
        Type::Str => Type::Arr(crate::ssa_lower::intern_arr_layout(
            ctx.arr_layouts,
            Type::Substr,
        )),
        Type::Arr(iid) if ctx.arr_layouts[iid.0 as usize] == Type::Str => {
            let inner = crate::ssa_lower::intern_arr_layout(ctx.arr_layouts, Type::Substr);
            Type::Arr(crate::ssa_lower::intern_arr_layout(
                ctx.arr_layouts,
                Type::Arr(inner),
            ))
        }
        _ => ty,
    }
}
