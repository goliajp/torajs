//! RFC 20260914-array-from-async-ts — `__torajs_ns_static_bind(id, f)`.
//!
//! A self-hosted namespace static's body is TypeScript the compiler
//! injects; its runtime ns-static cell has nothing of its own to run
//! and forwards to a closure the program registers. This synthetic
//! call is that registration, spelled by the injector as the
//! program's first statement: the cell's id is a literal off the
//! shared table (`torajs_rc::ns_static::ns_static_id`), the value an
//! arrow over the helper. The checker types the bare name through
//! `check_type_of_ident_synth`; this arm owns the lowering.
//!
//! The kernel takes its own stake on the closure (argv BORROWED), so
//! a fresh temp is released here like any other borrowed argument.

use crate::ast::{Expr, ExprId};

pub(crate) const NAME: &str = "__torajs_ns_static_bind";
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
    if n != NAME || args.len() != 2 {
        return None;
    }
    let Expr::Number(id) = ctx.ast.get_expr(args[0]) else {
        return None;
    };
    let id = *id as i64;
    let f_op = ctx.lower_expr(args[1]);
    let f_ty = ctx.operand_ty(&f_op);
    let boxed = ctx.box_to_any_from_expr(args[1], f_op);
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.ns_static_bind,
            vec![Operand::ConstI64(id), boxed],
        ),
        Type::Void,
        None,
    );
    if f_ty.is_refcounted() && ctx.expr_is_fresh_owned(args[1]) {
        ctx.emit_drop_value(f_op, f_ty);
    }
    Some(Operand::Value(v))
}
