//! Arguments past the ones a built-in constructor reads.

use crate::ast::ExprId;
use crate::ssa::Operand;
use crate::ssa_lower::LowerCtx;

/// §13.3.5 — ArgumentListEvaluation evaluates every argument before
/// Construct, so the ones past what the constructor reads are evaluated
/// and dropped. The operands already lowered (`read`) stay parked across
/// them: a throwing extra argument releases them on its edge.
pub(crate) fn lower_ignored_args(
    ctx: &mut LowerCtx<'_>,
    read: &[(ExprId, Operand)],
    extras: &[ExprId],
) {
    if extras.is_empty() {
        return;
    }
    let parked: Vec<Option<usize>> = read
        .iter()
        .map(|(eid, op)| ctx.park_owned_temp(*eid, op))
        .collect();
    for &a in extras {
        let op = ctx.lower_expr(a);
        ctx.release_owned_temp(a, &op);
    }
    for tok in parked.into_iter().rev() {
        ctx.unpark_owned_temp(tok);
    }
}
