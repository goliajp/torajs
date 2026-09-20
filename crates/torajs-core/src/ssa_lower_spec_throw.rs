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

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// Emit the decided throw for `eid`, or answer `None` when the checker
/// did not mark it.
///
/// The operands are lowered first and their values dropped on the
/// floor: evaluation order is part of the semantics, the values are
/// not (nothing downstream of an unconditional throw runs). The
/// returned shape is the null pointer for the same reason the TDZ read
/// returns one — the checker typed the expression `Any` so that the
/// enclosing expression still types out, and the value is unreachable.
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
    Some(Operand::ConstPtrNull)
}
