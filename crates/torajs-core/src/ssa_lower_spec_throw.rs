//! Band B emission — an operation whose throw the checker already
//! decided (`Ast::spec_throws`).
//!
//! `docs/language-spec/ch1-type-runtime-boundary.md` §1 uses `1n & 1`
//! as the case that separates this language from both of its
//! neighbours. §13.15.3 does ToNumeric on both operands and throws
//! when the resulting types disagree; here both types are known while
//! compiling. bun must run the two coercions and the comparison to
//! reach that conclusion, because it discarded the types and has no
//! other way; tsc reaches it and then declines to produce an
//! executable. What this module emits is the third answer: the
//! operands still evaluate (§13.15.3 evaluates them before it
//! coerces, so their side effects are observable), and then the throw
//! happens — with no type dispatch at all, because the dispatch was
//! settled at compile time.
//!
//! The message is the one the runtime kernel raises for the same pair,
//! so what moved is when the decision is made, not what the program
//! prints.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// Emit the decided throw for `eid`, or answer `None` when the checker
/// did not mark it.
///
/// The operands are lowered first and their values dropped on the
/// floor: evaluation order is part of the semantics, the values are
/// not (nothing downstream of an unconditional throw runs).
///
/// **The unreachable value still needs the right SHAPE.** It is never
/// read, but it is materialized: a consumer whose slot is an f64
/// cannot hold a pointer, and the backend says so
/// (`FPR materialization can't hold ConstPtrNull`). `+x` inside
/// `function f(x: bigint): number` is exactly that — the return slot
/// is f64 while the operand lane is not. So the shape is taken from
/// the type the checker recorded for this expression, the same table
/// every other lowering consults.
pub(crate) fn try_emit(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    operands: &[ExprId],
) -> Option<Operand> {
    let msg = ctx.ast.spec_throws.get(&eid)?.clone();
    for &operand in operands {
        let _ = ctx.lower_expr(operand);
    }
    let msg_str = ctx.intern_string_literal(msg.as_str());
    let raiser = ctx.intrinsics.throw_type_error_str;
    let cur_block = ctx.cur_block;
    let _ = ctx.f.append_inst(
        cur_block,
        InstKind::Call(raiser, vec![Operand::Value(msg_str)]),
        Type::Void,
        None,
    );
    ctx.emit_throw_check(None);
    Some(dead_value_shaped_like(ctx, eid))
}

/// Band B at a CALL position — the operand list §13.3.6.1 asks for,
/// then [`try_emit`].
///
/// That clause evaluates the callee's BASE and then the arguments
/// before the call itself, so those side effects are observable even
/// though the call was decided to throw: `Object.assign(null, f())`
/// still runs `f`. A namespace name (`Object`, `Math`) is not a
/// value expression in this lowering — there is nothing to evaluate
/// and nothing to preserve — so it stays out of the list.
pub(crate) fn try_emit_call(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    if !ctx.ast.spec_throws.contains_key(&eid) {
        return None;
    }
    let mut operands: Vec<ExprId> = Vec::new();
    if let Expr::Member { obj, .. } = ctx.ast.get_expr(callee)
        && !matches!(
            ctx.ast.get_expr(*obj),
            Expr::Ident(n) if crate::ast::free_vars_is_global_name(n)
        )
    {
        operands.push(*obj);
    }
    operands.extend(args.iter().copied());
    try_emit(ctx, eid, &operands)
}

/// A value of the right shape that is never read. Only the register
/// class matters — an f64-typed consumer needs a float, everything
/// else takes the null pointer the TDZ read already uses.
fn dead_value_shaped_like(ctx: &LowerCtx<'_>, eid: ExprId) -> Operand {
    match ctx.expr_types.get(&eid) {
        Some(crate::check::Type::Number) => Operand::ConstF64(0.0),
        _ => Operand::ConstPtrNull,
    }
}
