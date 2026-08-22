//! `__torajs_super_prop_call(base, key, this, [args…])` — the
//! SuperProperty CALL synthetic the class desugar emits for
//! `super[k](…)`.
//!
//! Four operands, all boxed into the any world: the super base, the
//! property key (a string literal for `super.m`, a runtime value for
//! `super[k]`), the receiver §13.3.6 wants, and the dense args pack
//! the rewrite built. The kernel owns the whole sequence — see
//! `torajs-anyvalue::super_prop_call`.
//!
//! Ownership follows the `__torajs_super_value` rule verbatim: a
//! borrow-shaped source takes a +1 before boxing, the boxed temp is
//! released after the call, and an operand already in the any world
//! rides through untouched.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_lower(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Option<Operand> {
    if args.len() != 4 {
        return None;
    }
    let mut ops: Vec<Operand> = Vec::with_capacity(4);
    let mut boxed: Vec<Operand> = Vec::with_capacity(4);
    for &a in args {
        let raw = ctx.lower_expr(a);
        let raw_ty = ctx.operand_ty(&raw);
        if raw_ty == Type::Any {
            ops.push(raw);
        } else {
            if matches!(ctx.ast.get_expr(a), Expr::Ident(_) | Expr::Member { .. })
                && raw_ty.is_refcounted()
            {
                ctx.emit_rc_inc(raw.clone());
            }
            let b = ctx.box_to_any_from_expr(a, raw);
            ops.push(b.clone());
            boxed.push(b);
        }
    }
    let cur_block = ctx.cur_block;
    let kernel = ctx.intrinsics.super_prop_call;
    let out = ctx
        .f
        .append_inst(cur_block, InstKind::Call(kernel, ops), Type::Any, None);
    for b in boxed {
        ctx.emit_drop_value(b, Type::Any);
    }
    ctx.emit_throw_check(None);
    Some(Operand::Value(out))
}
