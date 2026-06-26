//! `Symbol(desc?)` direct constructor call pulled out of
//! [`crate::ssa_lower::lower_expr_inner`] `Expr::Call` dispatch as
//! chunk-39 of the `Expr::Call` god-arm decomp (chunks 1-38 = ... +
//! namespace-obj method dispatch).
//!
//! T-13.a (v0.4.0) — `Symbol(desc?)` constructor (call, not `new`).
//! Returns `Type::Symbol`. `desc` is optional; missing argument lowers
//! to `Operand::ConstPtrNull` (the `__torajs_symbol_alloc` runtime
//! treats a NULL desc as no-description — `rc_inc` no-ops and `print`
//! formats as `Symbol()`).
//!
//! S308 — trailing args[1..] lowered-and-dropped per S272 idiom so
//! step()-style side-effect exprs fire per ES §20.4.1 trailing-arg
//! ignore (check.rs S308 already typecheck-dropped).
//!
//! Returns `Some(op)` on hit; `None` on miss (callee not
//! `Ident("Symbol")`). Distinct from the `Symbol.for(key)` global-
//! registry path (that one routes through namespace dispatch).

use crate::ast::{Expr, ExprId};
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
    if n != "Symbol" {
        return None;
    }
    let desc_op: Operand = if args.is_empty() {
        Operand::ConstPtrNull
    } else {
        let v = ctx.lower_expr(args[0]);
        ctx.consume_if_ident(args[0]);
        v
    };
    // S308 — lower-and-drop trailing args[1..] per S272 idiom so
    // step()-style side-effect exprs fire per ES §20.4.1 trailing-
    // arg ignore (check.rs S308 already typecheck-dropped).
    for &a in args.iter().skip(1) {
        let _ = ctx.lower_expr(a);
    }
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.symbol_alloc, vec![desc_op]),
        Type::Symbol,
        None,
    );
    Some(Operand::Value(v))
}
