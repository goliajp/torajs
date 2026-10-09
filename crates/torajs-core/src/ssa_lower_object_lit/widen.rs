//! Phase 2 of [`super`]'s object-literal lowering: the W4 field-width
//! widen. Moved verbatim; the rationale is in the parent's module doc.

use crate::ast::{ExprId, PropKey};
use crate::ssa::{Operand, StructId, Type};
use crate::ssa_lower::LowerCtx;

pub(super) fn apply_w4_widen(
    ctx: &mut LowerCtx<'_>,
    field_tys: &mut [(PropKey, Type)],
    field_vals: &mut [Operand],
    eid: ExprId,
    declared_hint: Option<StructId>,
) {
    let key = crate::num_width::SlotKey::Anon(eid.0);
    // W4 shape-join (rotation 371) — the literal's own verdict OR the
    // family verdict: same-shaped literals share a layout, so the
    // first registrant must already claim the joined slot width. A
    // literal that lands in a declared slot takes that slot's layout
    // instead of sharing one with its family, so only its own verdict
    // counts: the family's would widen an integral value only for the
    // declared narrow slot to truncate it straight back.
    let family = declared_hint.is_none();
    let shape: Vec<PropKey> = field_tys.iter().map(|(n, _)| n.clone()).collect();
    for (i, (fname, fty)) in field_tys.iter_mut().enumerate() {
        if *fty == Type::I64
            && (ctx.num_f64_slots.field_is_f64(&key, fname.clone())
                || (family && ctx.num_f64_slots.objlit_shape_field_is_f64(&shape, fname)))
        {
            *fty = Type::F64;
            let coerced = ctx.coerce_to_f64(field_vals[i]);
            field_vals[i] = coerced;
        }
    }
}
