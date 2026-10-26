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

/// Whether the function being lowered is sloppy. Strictness has a
/// second source below the program (§11.2.2): a `"use strict"`
/// directive makes a function strict inside a sloppy Script, and
/// class code is always strict — the parser materializes a directive
/// into every such body, so reading the body is enough
/// (`Ast::body_is_sloppy`), read once off the body being lowered. The
/// program's own goal answers for `main`. Keying on the program alone
/// (the delete lane's first cut) let `() => { "use strict"; o[0] = 1
/// }` and every class method's `super.x = v` take the soft kernel
/// inside an unflagged test262 script — nine cases went from pass to
/// bug in one sweep.
pub(crate) fn cur_fn_is_sloppy(ctx: &LowerCtx<'_>) -> bool {
    ctx.body_sloppy
}

/// Emit one [[Set]] call in the function's goal: `strict` (void, a
/// refusal records the TypeError) or `soft` (answers I64, the answer
/// is dropped — sloppy code does not observe it).
pub(crate) fn emit_any_set(
    ctx: &mut LowerCtx<'_>,
    strict: FuncId,
    soft: FuncId,
    args: Vec<Operand>,
) {
    let cur_block = ctx.cur_block;
    if cur_fn_is_sloppy(ctx) {
        ctx.f
            .append_inst(cur_block, InstKind::Call(soft, args), Type::I64, None);
    } else {
        ctx.f.append_void(cur_block, InstKind::Call(strict, args));
    }
}
