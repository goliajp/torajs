//! The split-count lane cut on a literal of one Latin-1 code unit
//! (`s.split(" ").length`): the call passes the separator as its byte,
//! so `__torajs_str_split_count_byte` reads only the receiver's header
//! instead of `__torajs_str_split_count` reading both and matching on
//! their widths. The receiver's width is a run-time fact and stays in
//! the kernel; a branch on it here measured +1.3M instructions per
//! `tr build` of a one-site program, for about ten instructions a call.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

const LEAF: &str = "__torajs_str_split_count_byte";

/// The byte a separator literal of exactly one Latin-1 code unit
/// spells.
pub(super) fn literal_byte(ctx: &LowerCtx<'_>, sep: ExprId) -> Option<u8> {
    let Expr::String(s) = ctx.ast.get_expr(sep) else {
        return None;
    };
    let mut units = s.code_units();
    let unit = units.next()?;
    if units.next().is_some() {
        return None;
    }
    u8::try_from(unit).ok()
}

/// Token count of `recv.split(sep)` for the literal `sep` spelling
/// `byte`.
pub(super) fn emit(ctx: &mut LowerCtx<'_>, recv: Operand, byte: u8) -> Operand {
    let leaf = *ctx
        .fn_table
        .get(LEAF)
        .expect("__torajs_str_split_count_byte intrinsic missing");
    let n = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(leaf, vec![recv, Operand::ConstI64(i64::from(byte))]),
        Type::I64,
        None,
    );
    Operand::Value(n)
}
