//! §13.15.2 PutValue has two goals, and a [[Set]] refusal is the one
//! place they differ: strict code turns `false` into a TypeError,
//! sloppy code discards it and the assignment expression still
//! answers its right-hand side. The kernels ship both flavors (the
//! `_soft` twins `Reflect.set` already rode); this is the one place
//! an assignment picks between them, keyed on the compile goal the
//! way `ssa_lower_delete` picks for `delete`.
//!
//! What stays strict in every goal: the nullish-base TypeError
//! (§13.15.2 step 1.b ToObject), a private-brand miss (§7.3.32 — a
//! TypeError, not a refusal), and a setter's own throw.

use crate::ssa::{FuncId, InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// Emit one [[Set]] call in the program's goal: `strict` (void, a
/// refusal records the TypeError) or `soft` (answers I64, the answer
/// is dropped — sloppy code does not observe it).
pub(crate) fn emit_any_set(
    ctx: &mut LowerCtx<'_>,
    strict: FuncId,
    soft: FuncId,
    args: Vec<Operand>,
) {
    let cur_block = ctx.cur_block;
    if ctx.ast.program_is_sloppy() {
        ctx.f
            .append_inst(cur_block, InstKind::Call(soft, args), Type::I64, None);
    } else {
        ctx.f.append_void(cur_block, InstKind::Call(strict, args));
    }
}
