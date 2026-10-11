//! The accumulator a seeded reduce hands back, cast into the object or
//! array type its callback's accumulator parameter was written with.
//!
//! The checker answers such a reduce with the accumulator's type `U`
//! (`check_type_of_call::reduce_acc::seeded_result`), also when the
//! callback's return could not be named or the seed is `any`. The lane
//! then ran the accumulator as a box; the box crosses into `U` through
//! the checked cast every annotated object slot takes, so a value of
//! another shape throws here instead of being read as `U`'s layout.
//! A scalar `U` is answered only when the return and the seed are
//! `U` themselves, so no scalar product reaches this cast.

use crate::ast::{Expr, ExprId, Stmt};
use crate::check::Type as CheckType;
use crate::ssa::{Operand, Type};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_checked_cast::CastSlot;

/// `out`, the product of the reduce at `eid` whose callback is `cb`
/// (typed `fn_ty`, leading `__this` when `promoted`), cast into the
/// accumulator's object or array type; `out` as it is otherwise.
pub(crate) fn cast_reduce_product(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    cb: ExprId,
    fn_ty: Type,
    promoted: bool,
    out: Operand,
) -> Operand {
    if ctx.operand_ty(&out) != Type::Any
        || !matches!(
            ctx.expr_types.get(&eid),
            Some(CheckType::Struct(_) | CheckType::Array(_))
        )
    {
        return out;
    }
    let Some(slot) = (ctx.sig_param_tys(fn_ty)).and_then(|p| p.get(usize::from(promoted)).copied())
    else {
        return out;
    };
    let Some(ann) = acc_param_ann(ctx, cb) else {
        return out;
    };
    let cast = match slot {
        Type::Obj(sid) => ctx.checked_cast_obj(out, sid, &ann, CastSlot::Cast),
        Type::Arr(_) => ctx.checked_cast_obj_arr(out, slot, &ann, CastSlot::Cast),
        _ => None,
    };
    let Some(v) = cast else { return out };
    ctx.emit_drop_value(out, Type::Any);
    v
}

/// The annotation of the accumulator parameter of the callback `cb`
/// names: a lifted closure's or a top-level function's first own
/// parameter.
fn acc_param_ann(ctx: &LowerCtx<'_>, cb: ExprId) -> Option<String> {
    let fn_name = match ctx.ast.get_expr(cb) {
        Expr::Closure { fn_name, .. } => fn_name.clone(),
        Expr::Ident(name) if !ctx.locals.contains_key(name) => name.clone(),
        _ => return None,
    };
    let params = ctx.ast.stmts.iter().find_map(|s| match s {
        Stmt::FnDecl { name, params, .. } if *name == fn_name => Some(params),
        _ => None,
    })?;
    let own = params
        .iter()
        .find(|p| !matches!(p.name.as_str(), "__env" | "__this" | "__torajs_argc"))?;
    own.type_ann.clone()
}
