//! Phase 2 of [`super`]'s object-literal lowering: the W4 field-width
//! widen. Moved verbatim; the rationale is in the parent's module doc.

use crate::ast::{ExprId, PropKey};
use crate::ssa::{Operand, Type};
use crate::ssa_lower::LowerCtx;

/// Widen the literal's integral number fields that its own alias class
/// proves may hold an f64.
///
/// Only the literal's own verdict counts. Its class is the class of
/// every slot it flows into, so a reader of any of those slots resolves
/// the same widths; a verdict borrowed from other same-shaped literals
/// would store f64 bits into a cell that a narrow declared reader
/// (`const ps: P[] = [{ x: 1 }]` beside an unrelated `q.x = 5.5`) loads
/// as an integer. Literals of differing widths do not share a layout:
/// the un-hinted layout scan never narrows an f64 field.
pub(super) fn apply_w4_widen(
    ctx: &mut LowerCtx<'_>,
    field_tys: &mut [(PropKey, Type)],
    field_vals: &mut [Operand],
    eid: ExprId,
) {
    let key = crate::num_width::SlotKey::Anon(eid.0);
    for (i, (fname, fty)) in field_tys.iter_mut().enumerate() {
        if *fty == Type::I64 && ctx.num_f64_slots.field_is_f64(&key, fname.clone()) {
            *fty = Type::F64;
            let coerced = ctx.coerce_to_f64(field_vals[i]);
            field_vals[i] = coerced;
        }
    }
}
