//! `new FinalizationRegistry(cleanupCallback)` — §26.2.1.1. Types
//! `Type::Any`: the registry is reached only through the any-lane
//! kernels, the same posture as the buffer family.

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// Every argument is evaluated (the borrowed pair covers the arity a
/// program writes); only the first reaches the kernel, which owns the
/// IsCallable TypeError.
pub(crate) fn lower(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Operand {
    let ops = crate::ssa_lower_new::lower_borrowed_any_pair(ctx, args);
    let cleanup = ops[0].0.clone();
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.finreg_new, vec![cleanup]),
        Type::Any,
        None,
    );
    crate::ssa_lower_new::release_borrowed_any_pair(ctx, ops);
    ctx.emit_throw_check(None);
    Operand::Value(v)
}
