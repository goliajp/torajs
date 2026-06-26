//! T-13.c (v0.4.0) — well-known `Symbol` singletons:
//! `Symbol.iterator`, `Symbol.asyncIterator`, `Symbol.toPrimitive`.
//! Each access lowers to a runtime helper call that lazy-inits the
//! process-level singleton + `rc_inc`'s for the caller. Pulled out
//! of [`crate::ssa_lower::lower_expr_inner`]'s `Expr::Member`
//! god-arm as chunk-59 of the decomp (chunks 1-58 = ... + Math /
//! Number / ctor builtin-namespace Member cluster).
//!
//! Returns `Some(op)` on hit; `None` on miss (callee `obj` not the
//! `Symbol` Ident, or `name` not one of the three wellknown
//! symbols).

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_lower(ctx: &mut LowerCtx<'_>, obj: ExprId, name: &str) -> Option<Operand> {
    let Expr::Ident(n) = ctx.ast.get_expr(obj) else {
        return None;
    };
    if n != "Symbol" {
        return None;
    }
    let fid = match name {
        "iterator" => ctx.intrinsics.symbol_iterator,
        "asyncIterator" => ctx.intrinsics.symbol_async_iterator,
        "toPrimitive" => ctx.intrinsics.symbol_to_primitive,
        _ => return None,
    };
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(fid, Vec::new()),
        Type::Symbol,
        None,
    );
    Some(Operand::Value(v))
}
