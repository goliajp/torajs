//! RFC 20260714-objlit-accessor blade 2 — the object-literal setter
//! write lane, split out of `ssa_lower_assign_member` (which the arm
//! pushed past the 500-line file limit).
//!
//! The getter read lane is its mirror, in `ssa_lower_member_obj_field`.

use crate::ast::ExprId;
use crate::ssa::{Operand, StructId, Type};
use crate::ssa_lower::LowerCtx;

/// RFC 20260714-objlit-accessor blade 2 — `o.b = v` where the literal
/// declared `set b(v) { ... }`. The setter closure sits in the layout
/// under `__setter_b`; invoke it with the receiver (blade 1's
/// `(__env, __this, ...user)` ABI) and evaluate to the assigned value,
/// as a plain Store would. Sound by construction — the accessor is a
/// field of the receiver's own type, not something reverse-looked-up
/// from a structurally-equal alias (RFC §2.1).
pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    obj_val: Operand,
    sid: StructId,
    field: &str,
    value: ExprId,
) -> Option<Operand> {
    let slot = format!("__setter_{field}");
    let layout = &ctx.struct_layouts[sid.0 as usize];
    let (idx, ty) = layout
        .iter()
        .enumerate()
        .find_map(|(i, (fname, fty))| (*fname == slot).then_some((i, *fty)))?;
    let Type::Closure(sig_id) = ty else {
        return None;
    };
    let offset = crate::ssa_lower::OBJ_HEADER_SIZE + idx as u64 * 8;
    crate::ssa_lower_call_struct_method_dispatch::emit_receiver_closure_call(
        ctx,
        obj_val,
        offset,
        sig_id,
        std::slice::from_ref(&value),
    );
    // An assignment evaluates to the assigned value, not to whatever the
    // setter body returned.
    Some(ctx.lower_expr(value))
}
