//! RFC 20260915 — the spec abstract operations the TypeScript the
//! compiler injects calls by bare name: `__torajs_is_constructor(v)`
//! (§7.2.4 IsConstructor) and `__torajs_promise_species_ctor(p)`
//! (§7.3.22 SpeciesConstructor(O, %Promise%)).
//!
//! `Array.fromAsync`'s self-hosted helper (RFC 20260914) spelled
//! IsConstructor as `typeof c === "function"`, which admits every
//! callable: a callable non-constructor |this| (an arrow, a builtin
//! static) was `new`ed instead of falling back to ArrayCreate
//! (proposal §2.1.1 steps 3.a / 7.a). The kernel is
//! `torajs-anyvalue::construct::__torajs_is_constructor` — a bound
//! function is judged through its target.
//!
//! The `finally` helper (`ast::inject_promise_finally`) cannot spell
//! SpeciesConstructor in source: a `.constructor` member read is an
//! opaque constructor mention to `builtin_proto_shadow`, and one such
//! read widens every typed builtin chain in the program to the any
//! lane (`p = p.then(...)` stopped building). The kernel is
//! `torajs-anyvalue::method_call_promise_invoke::__torajs_promise_species_ctor`;
//! its answer is owned — `undefined` for the default constructor,
//! else the species — and may leave a TypeError pending.
//!
//! Both: the checker types the name through `check_type_of_ident_synth`;
//! the argument is boxed to any and, when it was a fresh owned value,
//! released after the call (the kernels only read it).

use crate::ast::{Expr, ExprId};

pub(crate) const NAME: &str = "__torajs_is_constructor";
pub(crate) const SPECIES_NAME: &str = "__torajs_promise_species_ctor";
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
    let (intrinsic, ret_ty) = if n == NAME {
        (ctx.intrinsics.is_constructor, Type::Bool)
    } else if n == SPECIES_NAME {
        (ctx.intrinsics.promise_species_ctor, Type::Any)
    } else {
        return None;
    };
    if args.len() != 1 {
        return None;
    }
    let v_op = ctx.lower_expr(args[0]);
    let v_ty = ctx.operand_ty(&v_op);
    let boxed = ctx.box_to_any_from_expr(args[0], v_op);
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(intrinsic, vec![boxed]),
        ret_ty,
        None,
    );
    if v_ty.is_refcounted() && ctx.expr_is_fresh_owned(args[0]) {
        ctx.emit_drop_value(v_op, v_ty);
    }
    if ret_ty == Type::Any {
        ctx.emit_throw_check_owned(None, Operand::Value(v), Type::Any);
    }
    Some(Operand::Value(v))
}
