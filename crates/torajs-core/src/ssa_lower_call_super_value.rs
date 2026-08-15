//! RFC 20260815 knife 2b — `__torajs_super_value(P, this, [args…])`
//! synthetic call lowering.
//!
//! Three operands, all boxed to Any: the parent binding (an `any` let
//! by construction — the extracted `__ccp<N>` heritage value), the
//! subclass-allocated `this`, and the dense args pack the AST rewrite
//! built (an array literal at an explicit `super(a, b)` site, the
//! rest binding itself in the implicit forwarding ctor). The runtime
//! kernel (`__torajs_anyv_super_call`) owns the whole dispatch — a
//! class-cell parent routes to its registered `__ctorany_<P>` twin, a
//! closure takes [[Call]] with `this` bound, anything else raises
//! §15.7.14's TypeError.

use crate::ast::{Expr, ExprId};
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
    // `__torajs_heritage_check(P)` (rotation 410) — same synthetic
    // family, one boxed operand, void kernel; the throw check
    // surfaces §15.7.14 step 5's TypeError at definition time.
    if n == "__torajs_heritage_check" && args.len() == 1 {
        let raw = ctx.lower_expr(args[0]);
        let (op, boxed) = if ctx.operand_ty(&raw) == Type::Any {
            (raw, None)
        } else {
            // Same three-shape ownership rule as the dispatch below:
            // a borrow-shape source takes a +1 before boxing, and the
            // boxed temp is released after the kernel returns.
            if matches!(
                ctx.ast.get_expr(args[0]),
                Expr::Ident(_) | Expr::Member { .. }
            ) && ctx.operand_ty(&raw).is_refcounted()
            {
                ctx.emit_rc_inc(raw.clone());
            }
            let b = ctx.box_to_any_from_expr(args[0], raw);
            (b.clone(), Some(b))
        };
        let cur_block = ctx.cur_block;
        let kernel = ctx.intrinsics.heritage_check;
        ctx.f
            .append_void(cur_block, InstKind::Call(kernel, vec![op]));
        if let Some(b) = boxed {
            ctx.emit_drop_value(b, Type::Any);
        }
        ctx.emit_throw_check(None);
        return Some(Operand::ConstPtrNull);
    }
    if n != "__torajs_super_value" || args.len() != 3 {
        return None;
    }
    let mut ops: Vec<Operand> = Vec::with_capacity(3);
    let mut boxed: Vec<Option<Operand>> = Vec::with_capacity(3);
    for &a in args {
        let raw = ctx.lower_expr(a);
        let raw_ty = ctx.operand_ty(&raw);
        if raw_ty == Type::Any {
            ops.push(raw);
            boxed.push(None);
        } else {
            // The pack literal lowers as a typed Arr temp; boxing
            // TRANSFERS the temp's reference, released after the
            // call (same three-shape rule the NewDynamic lowering
            // follows).
            if matches!(ctx.ast.get_expr(a), Expr::Ident(_) | Expr::Member { .. })
                && raw_ty.is_refcounted()
            {
                ctx.emit_rc_inc(raw.clone());
            }
            let b = ctx.box_to_any_from_expr(a, raw);
            ops.push(b.clone());
            boxed.push(Some(b));
        }
    }
    let cur_block = ctx.cur_block;
    let kernel = ctx.intrinsics.super_call_value;
    let result = ctx
        .f
        .append_inst(cur_block, InstKind::Call(kernel, ops), Type::Any, None);
    for b in boxed.into_iter().flatten() {
        ctx.emit_drop_value(b, Type::Any);
    }
    ctx.emit_throw_check(None);
    Some(Operand::Value(result))
}
