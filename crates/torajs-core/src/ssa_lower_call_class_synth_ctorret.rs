//! RFC 20260820-ctor-return-override — the §10.2.2 step 13 arms of
//! [`crate::ssa_lower_call_class_synth`], in their own sibling to keep
//! the parent under its size cap (it has ten lines of room left).
//!
//! Two synthetic call names the class desugar emits, and only for the
//! classes `Ast::ctor_return_override` names:
//!
//! - `__torajs_ctor_ret_value(incumbent, v)` — the §10.2.2 step 13
//!   pick, at both the `super(…)` site and the factory.
//! - `__torajs_ctor_ret_carry(minted, target, "<name>")` — one of the
//!   class's own elements moved onto an adopted object.
//!
//! The pick answers an OWNED box, so the enclosing assignment or
//! return takes it like any other owned temp; the carry borrows all
//! three and answers nothing.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// Name dispatch for the family — one prefix arm in the parent
/// reaches all three, which is what keeps that file's last few lines
/// of headroom intact.
pub(crate) fn try_lower(ctx: &mut LowerCtx<'_>, name: &str, args: &[ExprId]) -> Option<Operand> {
    match name {
        "__torajs_ctor_ret_value" => try_lower_value(ctx, args),
        "__torajs_ctor_ret_carry" => try_lower_carry(ctx, args),
        _ => None,
    }
}

/// See module doc.
fn try_lower_value(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Option<Operand> {
    if args.len() != 2 {
        return None;
    }
    let a = ctx.lower_expr(args[0]);
    let a = ctx.box_to_any(a);
    let b = ctx.lower_expr(args[1]);
    let b = ctx.box_to_any(b);
    let fid = ctx.intrinsics.ctor_ret_value;
    let cur_block = ctx.cur_block;
    let out = ctx
        .f
        .append_inst(cur_block, InstKind::Call(fid, vec![a, b]), Type::Any, None);
    Some(Operand::Value(out))
}

/// See module doc. The element name is a literal the desugar wrote,
/// so it interns at compile time rather than riding as a runtime
/// string — the same channel every other statically-known member name
/// takes.
fn try_lower_carry(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Option<Operand> {
    if args.len() != 3 {
        return None;
    }
    let Expr::String(name) = ctx.ast.get_expr(args[2]) else {
        return None;
    };
    let name = name.clone();
    let minted = ctx.lower_expr(args[0]);
    let minted = ctx.box_to_any(minted);
    let target = ctx.lower_expr(args[1]);
    let target = ctx.box_to_any(target);
    let key = ctx.intern_string_literal(&name);
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.ctor_ret_carry,
            vec![minted, target, Operand::Value(key)],
        ),
    );
    Some(Operand::ConstI64(0))
}
