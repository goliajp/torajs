//! Arms the checker joined to an object type: both store the pointer
//! itself in the result slot, unboxed.

use crate::ast::ExprId;
use crate::check::Type as T;
use crate::ssa::{BlockId, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// A class instance against `null`: the checker joins it to
/// Nullable(ClassRef), not Any (only Number / Boolean / Struct widen,
/// check_type_compare), and a nullable class slot holds the null
/// pointer natively, as `let v: C | null = null` does. Boxing it here
/// handed an Any to the `C | null` binding, which stored the box bits:
/// `v === null` never held for the null arm and the next member read
/// dereferenced the box.
pub(super) fn class_or_null(
    ctx: &LowerCtx<'_>,
    eid: ExprId,
    (tt, et): (Type, Type),
    (then_null, else_null): (bool, bool),
) -> Option<Type> {
    if then_null == else_null || !matches!(ctx.expr_types.get(&eid), Some(T::Nullable(_))) {
        return None;
    }
    let res = if then_null { et } else { tt };
    matches!(res, Type::Obj(_)).then_some(res)
}

/// A conditional in a declared position whose arms the checker typed
/// as the declared object type (`check_assignable` `declared_join`):
/// `c ? undefined : new Sq()` under `Shape | undefined`, or two
/// unrelated classes under an interface. Each arm is an object pointer
/// or a nullish literal; the slot takes the pointer as it is, an
/// `undefined` arm as the width's undefined cell (`null` is the null
/// pointer already). Answers the arm values and the slot's repr — the
/// first object arm's; every reader goes by the declared type, which
/// both pointers satisfy.
pub(super) fn declared(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    (then_val, else_val): (Operand, Operand),
    (then_end, else_end): (&mut BlockId, &mut BlockId),
    (then_branch, else_branch): (ExprId, ExprId),
) -> Option<(Operand, Operand, Type)> {
    let inner = match ctx.expr_types.get(&eid)? {
        T::Nullable(t) => &**t,
        t => t,
    };
    if !matches!(inner, T::Struct(_) | T::ClassRef(_)) {
        return None;
    }
    let (bt, be) = (
        ctx.expr_types.get(&then_branch)?,
        ctx.expr_types.get(&else_branch)?,
    );
    if bt == be {
        return None;
    }
    let nullish = |t: &T| matches!(t, T::Undefined | T::Null);
    let (then_nullish, else_nullish) = (nullish(bt), nullish(be));
    let (then_undef, else_undef) = (*bt == T::Undefined, *be == T::Undefined);
    let (tt, et) = (ctx.operand_ty(&then_val), ctx.operand_ty(&else_val));
    let arm_fits = |nullish: bool, ty: &Type| nullish || matches!(ty, Type::Obj(_));
    if !arm_fits(then_nullish, &tt) || !arm_fits(else_nullish, &et) {
        return None;
    }
    let res = match (then_nullish, else_nullish) {
        (false, _) => tt,
        (true, false) => et,
        (true, true) => return None,
    };
    let undef_arm = |ctx: &mut LowerCtx<'_>, end: &mut BlockId| {
        ctx.cur_block = *end;
        let cell = ctx
            .str_undef_sentinel_for(res)
            .expect("an object slot spells undefined with the generic cell");
        *end = ctx.cur_block;
        cell
    };
    let then_val = if then_undef {
        undef_arm(ctx, then_end)
    } else {
        then_val
    };
    let else_val = if else_undef {
        undef_arm(ctx, else_end)
    } else {
        else_val
    };
    Some((then_val, else_val, res))
}
