//! RFC 20260815 knife 2b — `__torajs_super_value(P, this, [args…])`
//! synthetic call lowering.
//!
//! Four operands, all boxed to Any: the parent binding (an `any` let
//! by construction — the extracted `__ccp<N>` heritage value), the
//! subclass-allocated `this`, the dense args pack the AST rewrite
//! built (an array literal at an explicit `super(a, b)` site, the
//! rest binding itself in the implicit forwarding ctor), and the
//! subclass constructor's `new.target`, which a plain-function parent
//! receives (§10.2.1.1 via its construct face). The runtime
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
            let is_borrow = matches!(
                ctx.ast.get_expr(args[0]),
                Expr::Ident(_) | Expr::Member { .. }
            );
            let b = ctx.box_owned_from_expr(args[0], raw, is_borrow);
            (b, Some(b))
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
    // §10.2.1 step 2 — the capturing lane's [[Call]] guard: the
    // message is known while compiling (bun's, with the class name the
    // site carries as a literal), so the raise is the compiler-message
    // one.
    if n == crate::ast::capturing_classes::new_target::CALL_THROW
        && let [name] = args
        && let Expr::String(shown) = ctx.ast.get_expr(*name)
    {
        let text = format!("Cannot call a class constructor {shown} without |new|");
        let msg = ctx.intern_string_literal(text.as_str());
        let raiser = ctx.intrinsics.throw_type_error_str;
        let cur_block = ctx.cur_block;
        ctx.f
            .append_void(cur_block, InstKind::Call(raiser, vec![Operand::Value(msg)]));
        ctx.emit_throw_check(None);
        return Some(Operand::ConstPtrNull);
    }
    if n != "__torajs_super_value" || args.len() != 4 {
        return None;
    }
    let mut ops: Vec<Operand> = Vec::with_capacity(4);
    let mut boxed: Vec<Option<Operand>> = Vec::with_capacity(4);
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
            let is_borrow = matches!(ctx.ast.get_expr(a), Expr::Ident(_) | Expr::Member { .. });
            let b = ctx.box_owned_from_expr(a, raw, is_borrow);
            ops.push(b);
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
