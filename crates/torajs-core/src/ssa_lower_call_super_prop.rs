//! `__torajs_super_prop_get(base, key, this)`,
//! `__torajs_super_prop_set(base, key, value, this)`,
//! `__torajs_super_prop_call(base, key, this, [args…])` and the
//! `__torajs_super_prop_{incr,decr}_{post,pre}(base, key, this)`
//! quartet — the SuperProperty synthetics the desugars emit for
//! `super[k]`, `super[k] = v`, `super[k](…)` and `super[k]++`'s four
//! spellings (the update flags ride the callee name and become two
//! trailing i64 constants on the one update kernel).
//!
//! Every operand boxes into the any world: the super base, the
//! property key (a string literal for `super.m`, a runtime value for
//! `super[k]`), the receiver §13.3.7 wants, and — for the call — the
//! dense args pack the rewrite built. The kernels own the whole
//! sequence; see `torajs-anyvalue::super_prop_call`.
//!
//! Ownership follows the `__torajs_super_value` rule verbatim: a
//! borrow-shaped source takes a +1 before boxing, the boxed temp is
//! released after the call, and an operand already in the any world
//! rides through untouched.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_lower(ctx: &mut LowerCtx<'_>, name: &str, args: &[ExprId]) -> Option<Operand> {
    let (kernel, arity, flags) = match name {
        "__torajs_super_prop_get" => (ctx.intrinsics.super_prop_get, 3, None),
        "__torajs_super_prop_set" => (ctx.intrinsics.super_prop_set, 4, None),
        "__torajs_super_prop_call" => (ctx.intrinsics.super_prop_call, 4, None),
        "__torajs_super_prop_opt_call" => (ctx.intrinsics.super_prop_opt_call, 4, None),
        "__torajs_super_prop_incr_post" => (ctx.intrinsics.super_prop_update, 3, Some((1, 0))),
        "__torajs_super_prop_incr_pre" => (ctx.intrinsics.super_prop_update, 3, Some((1, 1))),
        "__torajs_super_prop_decr_post" => (ctx.intrinsics.super_prop_update, 3, Some((0, 0))),
        "__torajs_super_prop_decr_pre" => (ctx.intrinsics.super_prop_update, 3, Some((0, 1))),
        _ => return None,
    };
    if args.len() != arity {
        return None;
    }
    let mut ops: Vec<Operand> = Vec::with_capacity(arity);
    let mut boxed: Vec<Operand> = Vec::with_capacity(arity);
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
    if let Some((is_inc, is_prefix)) = flags {
        ops.push(Operand::ConstI64(is_inc));
        ops.push(Operand::ConstI64(is_prefix));
    }
    let cur_block = ctx.cur_block;
    let out = ctx
        .f
        .append_inst(cur_block, InstKind::Call(kernel, ops), Type::Any, None);
    for b in boxed {
        ctx.emit_drop_value(b, Type::Any);
    }
    ctx.emit_throw_check(None);
    Some(Operand::Value(out))
}
