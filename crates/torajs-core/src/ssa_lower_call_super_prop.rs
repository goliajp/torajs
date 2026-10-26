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
//! rides through — released after the call when it is an owned temp
//! (`Object.getPrototypeOf(home)`, the runtime base of a unit class or
//! of a program that re-links prototypes, leaked one reference to the
//! home per site visit).

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_lower(ctx: &mut LowerCtx<'_>, name: &str, args: &[ExprId]) -> Option<Operand> {
    let (kernel, arity, flags) = match name {
        "__torajs_super_prop_get" => (ctx.intrinsics.super_prop_get, 3, None),
        // Not a SuperProperty, the same operand discipline: `(this,
        // private name, value)`, all any-world, all borrowed.
        crate::ast::private_brand::FIELD_DEFINE => (ctx.intrinsics.priv_field_define, 3, None),
        // Nor this: `(f, this, [args…])`, a callee called on the receiver
        // it was read from, which is Reflect.apply's kernel.
        "__torajs_call_with_this" => (ctx.intrinsics.reflect_apply, 3, None),
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
    // §13.3.7.1 evaluates the key expression before
    // MakeSuperPropertyReference reads the home object's prototype, so
    // a key that re-links the prototype chain sees the new base.
    let order: &[usize] = if matches!(
        name,
        crate::ast::private_brand::FIELD_DEFINE | "__torajs_call_with_this"
    ) {
        &[0, 1, 2]
    } else if arity == 3 {
        &[1, 0, 2]
    } else {
        &[1, 0, 2, 3]
    };
    let mut ops: Vec<Operand> = vec![Operand::ConstI64(0); arity];
    let mut boxed: Vec<Operand> = Vec::with_capacity(arity);
    let mut any_temps: Vec<(ExprId, Operand)> = Vec::new();
    for &i in order {
        let a = args[i];
        let raw = ctx.lower_expr(a);
        let raw_ty = ctx.operand_ty(&raw);
        if raw_ty == Type::Any {
            any_temps.push((a, raw));
            ops[i] = raw;
        } else {
            let is_borrow = matches!(ctx.ast.get_expr(a), Expr::Ident(_) | Expr::Member { .. });
            let b = ctx.box_owned_from_expr(a, raw, is_borrow);
            ops[i] = b;
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
    for (a, op) in any_temps {
        ctx.release_owned_temp(a, &op);
    }
    ctx.emit_throw_check(None);
    Some(Operand::Value(out))
}
