//! `Object(x)` callable coercion — RFC 20260716-primitive-wrapper-
//! substrate 刀 4. ES §20.1.1.1: primitive → fresh wrapper, heap
//! object → identity, null/undef → fresh `{}`. Emits
//! `Call(any_to_object, boxed_arg)`; the runtime kernel decides the
//! per-tag mapping so one dispatch table owns the ToObject policy.
//!
//! Trailing args are lowered-and-dropped (S307 idiom, same as
//! Number/String/Boolean callable coercion). 0-arg calls forward a
//! boxed `undefined` so the runtime's null/undef arm supplies the
//! empty-object rule.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// Try to lower an `Object(x)` callable coercion. Returns `Some` when
/// dispatched (any arg shape lands, arg's static type doesn't gate).
pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let Expr::Ident(n) = ctx.ast.get_expr(callee) else {
        return None;
    };
    if n != "Object" {
        return None;
    }
    // S307 — trailing args[1..] lowered so step()-style side effects
    // fire; ES §20.1.1.1 reads only args[0].
    for &a in args.iter().skip(1) {
        let _ = ctx.lower_expr(a);
    }
    let arg_any = if let Some(&a) = args.first() {
        let val = ctx.lower_expr(a);
        ctx.box_to_any_from_expr(a, val)
    } else {
        // `Object()` — spec §20.1.1.1 step 1a: value is undefined →
        // OrdinaryObjectCreate. Route through the same kernel with
        // a boxed ANY_UNDEF so the null/undef arm mints the object.
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.any_box,
                vec![Operand::ConstI64(5), Operand::ConstI64(0)],
            ),
            Type::Any,
            None,
        );
        Operand::Value(v)
    };
    let out = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.any_to_object, vec![arg_any]),
        Type::Any,
        None,
    );
    Some(Operand::Value(out))
}
