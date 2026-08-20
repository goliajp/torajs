//! RFC 20260820-ctor-return-override — the §10.2.2 step 13 arms of
//! [`crate::ssa_lower_call_class_synth`], in their own sibling to keep
//! the parent under its size cap (it has ten lines of room left).
//!
//! Two synthetic call names the class desugar emits, and only for the
//! classes `Ast::ctor_return_override` names:
//!
//! - `__torajs_ctor_ret_value(incumbent, v, derived)` — the §10.2.2
//!   step 13 pick, at both the `super(…)` site and the factory.
//! - `__torajs_ctor_ret_carry(minted, target, "<name>")` — one of the
//!   class's own elements moved onto an adopted object.
//!
//! The pick is borrow-shaped; the carry borrows all three operands
//! and answers nothing.
//!
//! Every operand rides `any_arg`, which boxes only what is not
//! already an any. Boxing one that is re-wraps a whole nanbox as if
//! it were a bare pointer — and for a HEAP payload that happens to be
//! the identity, so the mistake hides completely behind object
//! operands and only shows when a constructor answers a primitive
//! (`class B { constructor() { return 5 } }` read back as a Str).
//! Neither the gate nor the fixtures could see it.

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

/// Lower one operand into the any world, boxing only when it is not
/// already there. The checker's type for the expression is what
/// answers that — `box_to_any` reads the SSA slot type, which reports
/// `Ptr` for an any-typed load and so silently takes the pointer arm.
fn any_arg(ctx: &mut LowerCtx<'_>, eid: ExprId) -> Operand {
    let v = ctx.lower_expr(eid);
    if matches!(ctx.expr_types.get(&eid), Some(crate::check::Type::Any)) {
        v
    } else {
        ctx.box_to_any(v)
    }
}

/// See module doc.
fn try_lower_value(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Option<Operand> {
    if args.len() != 3 {
        return None;
    }
    let Expr::Bool(derived) = ctx.ast.get_expr(args[2]) else {
        return None;
    };
    let derived = Operand::ConstI64(i64::from(*derived));
    let a = any_arg(ctx, args[0]);
    let b = any_arg(ctx, args[1]);
    let fid = ctx.intrinsics.ctor_ret_value;
    let cur_block = ctx.cur_block;
    let out = ctx.f.append_inst(
        cur_block,
        InstKind::Call(fid, vec![a, b, derived]),
        Type::Any,
        None,
    );
    // Step 13.c raises for a derived constructor answering a
    // non-object; the kernel records it and still answers a live
    // object, so this check is what actually ends the path.
    ctx.emit_throw_check(None);
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
    let minted = any_arg(ctx, args[0]);
    let target = any_arg(ctx, args[1]);
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
