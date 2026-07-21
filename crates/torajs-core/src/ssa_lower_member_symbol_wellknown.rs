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

/// §6.1.5.1 well-known property names, ALPHABETICAL — index feeds
/// the runtime slot table (torajs-str `WELL_KNOWN_SLOTS`, lockstep).
pub(crate) const WELL_KNOWN_SYMBOL_NAMES: [&str; 15] = [
    "asyncDispose",
    "asyncIterator",
    "dispose",
    "hasInstance",
    "isConcatSpreadable",
    "iterator",
    "match",
    "matchAll",
    "replace",
    "search",
    "species",
    "split",
    "toPrimitive",
    "toStringTag",
    "unscopables",
];

pub(crate) fn try_lower(ctx: &mut LowerCtx<'_>, obj: ExprId, name: &str) -> Option<Operand> {
    let Expr::Ident(n) = ctx.ast.get_expr(obj) else {
        return None;
    };
    if n != "Symbol" {
        return None;
    }
    // The original trio keeps its dedicated zero-arg intrinsics
    // (same slot-table identity — the runtime externs delegate);
    // the rest ride the indexed `__torajs_symbol_well_known`
    // (RFC 20260722-builtin-proto-reflection 刀 2).
    let (fid, args) = match name {
        "iterator" => (ctx.intrinsics.symbol_iterator, Vec::new()),
        "asyncIterator" => (ctx.intrinsics.symbol_async_iterator, Vec::new()),
        "toPrimitive" => (ctx.intrinsics.symbol_to_primitive, Vec::new()),
        _ => {
            let idx = WELL_KNOWN_SYMBOL_NAMES.iter().position(|w| *w == name)?;
            (
                ctx.intrinsics.symbol_well_known,
                vec![Operand::ConstI64(idx as i64)],
            )
        }
    };
    let cur_block = ctx.cur_block;
    let v = ctx
        .f
        .append_inst(cur_block, InstKind::Call(fid, args), Type::Symbol, None);
    Some(Operand::Value(v))
}
