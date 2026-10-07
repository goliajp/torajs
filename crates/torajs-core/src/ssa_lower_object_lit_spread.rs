//! `{ ...src, k: v }` — how a spread in an object literal unfolds
//! into fields, moved out of [`crate::ssa_lower_object_lit`] whole.
//!
//! That file answers "what does one field of this literal lower
//! to"; this one answers a question a field never asks — which
//! fields a source struct even contributes. ES §7.3.25
//! CopyDataProperties reaches each own key through [[Get]], so an
//! accessor source contributes the getter's RESULT, and the
//! destructuring-rest desugar's omit list drops keys before either
//! a load or a retain happens.
//!
//! Pure position — the item is byte-for-byte what it was, and the
//! parent calls it under the same name.

use crate::ast::{ExprId, PropKey};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::{LowerCtx, OBJ_HEADER_SIZE};

/// `omit` — the destructuring-rest desugar's excluded keys
/// (chunk 707, decoded from the `__spread_omit__:` sentinel);
/// omitted fields are neither loaded nor rc-inc'd. Answers the
/// source cell and its layout.
pub(crate) fn unfold_spread(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    omit: &[PropKey],
    field_tys: &mut Vec<(PropKey, Type)>,
    field_vals: &mut Vec<Operand>,
) -> (Operand, crate::ssa::StructId) {
    let src_op = ctx.lower_expr(eid);
    let src_ty = ctx.operand_ty(&src_op);
    let Type::Obj(sid) = src_ty else {
        panic!("ssa-lower: object spread source must be a struct, got {src_ty:?}");
    };
    let layout = ctx.struct_layouts[sid.0 as usize].clone();
    for (idx, (sn, st)) in layout.iter().enumerate() {
        if crate::check_type_of_object_lit::private_slot(sn) {
            continue;
        }
        let off = OBJ_HEADER_SIZE + (idx as u64) * 8;
        // RFC 20260714-objlit-accessor blade 3 — CopyDataProperties
        // (ES §7.3.25) reaches each own key through [[Get]], so a source
        // accessor contributes the getter's RESULT as a DATA property.
        // Loading the `__getter_b` slot would have copied the getter
        // CLOSURE across instead of calling it.
        let (name, ty, val) = match crate::check_type_of_object_lit::accessor_slot(sn) {
            // A setter is not a source of data: its property already came
            // from the paired getter. (Recorded gap: a LONE setter should
            // read `undefined`; here it drops out of the copy.)
            Some(("__setter_", _)) => continue,
            Some(("__getter_", prop)) => {
                if omit.iter().any(|o| o == prop) {
                    continue;
                }
                let Type::Closure(sig_id) = *st else {
                    panic!(
                        "ssa-lower: accessor slot `{}` is not a closure (got {st:?})",
                        sn.lossy()
                    );
                };
                let ret = ctx.fn_sigs[sig_id.0 as usize].1;
                // Owned already — the getter returns its own value, so no
                // rc_inc (unlike the borrowed field load below).
                let lane =
                    crate::ssa_lower_call_struct_method_dispatch::objlit_slot::emit_getter_call;
                let v = lane(ctx, Some(eid), src_op, off, sig_id);
                (PropKey::from(prop), ret, v)
            }
            _ => {
                if omit.contains(sn) {
                    continue;
                }
                let cur_block = ctx.cur_block;
                let v = ctx
                    .f
                    .append_inst(cur_block, InstKind::Load(*st, src_op, off), *st, None);
                if st.is_refcounted() {
                    ctx.emit_rc_inc(Operand::Value(v));
                }
                (sn.clone(), *st, Operand::Value(v))
            }
        };
        if let Some(pos) = field_tys.iter().position(|(k, _)| *k == name) {
            field_tys[pos] = (name, ty);
            field_vals[pos] = val;
        } else {
            field_tys.push((name, ty));
            field_vals.push(val);
        }
    }
    (src_op, sid)
}
