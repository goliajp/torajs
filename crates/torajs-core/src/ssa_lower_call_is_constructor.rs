//! RFC 20260915 — `__torajs_is_constructor(v)`: spec IsConstructor for
//! the TypeScript the compiler injects.
//!
//! `Array.fromAsync`'s self-hosted helper (RFC 20260914) spelled
//! IsConstructor as `typeof c === "function"`, which admits every
//! callable: a callable non-constructor |this| (an arrow, a builtin
//! static) was `new`ed instead of falling back to ArrayCreate
//! (proposal §2.1.1 steps 3.a / 7.a). The kernel is
//! `torajs-anyvalue::construct::__torajs_is_constructor` — a bound
//! function is judged through its target — and the intrinsic was
//! already declared for the `new` lane; this arm gives injected source
//! a bare name for it. The checker types the name through
//! `check_type_of_ident_synth`; the argument is boxed to any and, when
//! it was a fresh owned value, released after the call (the kernel
//! only reads it).

use crate::ast::{Expr, ExprId};

pub(crate) const NAME: &str = "__torajs_is_constructor";
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let Expr::Ident(n) = ctx.ast.get_expr(callee) else {
        return None;
    };
    if n != NAME || args.len() != 1 {
        return None;
    }
    let v_op = ctx.lower_expr(args[0]);
    let v_ty = ctx.operand_ty(&v_op);
    let boxed = ctx.box_to_any_from_expr(args[0], v_op.clone());
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.is_constructor, vec![boxed]),
        Type::Bool,
        None,
    );
    if v_ty.is_refcounted() && ctx.expr_is_fresh_owned(args[0]) {
        ctx.emit_drop_value(v_op, v_ty);
    }
    Some(Operand::Value(v))
}
