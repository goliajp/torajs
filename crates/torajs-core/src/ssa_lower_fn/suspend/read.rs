//! The read of an awaited cell's settled value word (`super`).

use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// The settled cell's value word as `want_repr` asks for it; a
/// rejected source raises its reason here.
pub(super) fn read_settled(
    ctx: &mut LowerCtx<'_>,
    settled: Operand,
    want_repr: i64,
) -> crate::ssa::ValueId {
    let cb = ctx.cur_block;
    let raw = ctx.f.append_inst(
        cb,
        InstKind::Call(
            ctx.intrinsics.promise_get_value_as,
            vec![settled, Operand::ConstI64(want_repr)],
        ),
        Type::I64,
        None,
    );
    ctx.emit_throw_check_owned(None, settled, Type::Promise);
    raw
}
