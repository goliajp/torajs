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
use crate::check::Type as CType;
use crate::ssa::{BinOp, IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::{LowerCtx, OBJ_HEADER_SIZE};

/// `omit` — the destructuring-rest desugar's excluded keys
/// (chunk 707, decoded from the `__spread_omit__:` sentinel);
/// omitted fields are neither loaded nor rc-inc'd. Answers the
/// source cell and its layout.
///
/// A source field some literal leaves out may be absent from the
/// source cell (`ssa_lower_object_lit::absent`), and CopyDataProperties
/// copies only the keys the source owns: such a field contributes its
/// value only when present, and `absent` collects, per name, whether
/// every contributor so far lacked it (I64 1 / 0). `lit` is the
/// literal the spread is a member of.
pub(crate) fn unfold_spread(
    ctx: &mut LowerCtx<'_>,
    (lit, eid): (ExprId, ExprId),
    omit: &[PropKey],
    field_tys: &mut Vec<(PropKey, Type)>,
    field_vals: &mut Vec<Operand>,
    absent: &mut Vec<(PropKey, Operand)>,
) -> (Operand, crate::ssa::StructId) {
    crate::ssa_lower_iface::refuse_spread(ctx, eid);
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
                if may_be_absent(ctx, eid, sn) {
                    let field = (lit, sn, *st);
                    let fields = (&mut *field_tys, &mut *field_vals);
                    let (ty, val) =
                        maybe_absent(ctx, src_op, field, Operand::Value(v), fields, absent);
                    (sn.clone(), ty, val)
                } else {
                    if st.is_refcounted() {
                        ctx.emit_rc_inc(Operand::Value(v));
                    }
                    absent.retain(|(n, _)| n != sn);
                    (sn.clone(), *st, Operand::Value(v))
                }
            }
        };
        if !may_be_absent(ctx, eid, &name) {
            absent.retain(|(n, _)| *n != name);
        }
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

/// Whether the source `src` may lack its field `name`: the source's
/// type declares it optional, or some literal leaves a field of that
/// name out.
fn may_be_absent(ctx: &LowerCtx<'_>, src: ExprId, name: &PropKey) -> bool {
    matches!(ctx.expr_types.get(&src), Some(CType::Struct(fs)) if fs.is_optional(name))
        || ctx.ast.absent_field_names.contains(name)
}

/// The contribution of a source field that may be absent: the loaded
/// value when the source owns the field, else whatever an earlier
/// member put there. When the two slot types differ the field takes
/// the any lane, which the checker gave it too (a union of the two);
/// a literal whose field the checker typed otherwise is refused.
fn maybe_absent(
    ctx: &mut LowerCtx<'_>,
    src: Operand,
    field: (ExprId, &PropKey, Type),
    loaded: Operand,
    fields: (&mut Vec<(PropKey, Type)>, &mut Vec<Operand>),
    absent: &mut Vec<(PropKey, Operand)>,
) -> (Type, Operand) {
    let (lit, name, st) = field;
    let (field_tys, field_vals) = fields;
    let prev = field_tys.iter().position(|(k, _)| k == name);
    let (ty, prev_val) = match prev {
        None => (st, None),
        // a `null` the slot spells as its own null pointer
        Some(pos) if field_tys[pos].1 == st || null_fits(field_vals[pos], st) => {
            (st, Some(field_vals[pos]))
        }
        Some(pos) => {
            if st != Type::Any && !lit_field_is_any(ctx, lit, name) {
                panic!(
                    "ssa-lower: spreading a possibly absent `{}` over an earlier value of \
                     another type is not yet supported",
                    name.lossy()
                );
            }
            (Type::Any, Some(ctx.box_to_any(field_vals[pos])))
        }
    };
    let deleted = crate::ssa_lower_object_lit::absent::field_deleted(ctx, src, name);
    match prev_val {
        None => absent.push((name.clone(), deleted)),
        Some(_) => {
            if let Some(slot) = absent.iter_mut().find(|(n, _)| n == name) {
                let both = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::BinOp(BinOp::And, slot.1, deleted),
                    Type::I64,
                    None,
                );
                slot.1 = Operand::Value(both);
            }
        }
    }
    let is_del = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Ne, deleted, Operand::ConstI64(0)),
        Type::Bool,
        None,
    );
    // the field keeps the earlier value unless the source owns it; the
    // share is taken only for a value the copy keeps
    let slot = prev_val.map(|p| {
        let slot = ctx.alloca_in_entry(ty, Some("__spread_field"));
        ctx.f
            .append_void(ctx.cur_block, InstKind::Store(p, Operand::Value(slot), 0));
        slot
    });
    let own_blk = ctx.f.add_block();
    let merge = ctx.f.add_block();
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(is_del),
            then_blk: merge,
            else_blk: own_blk,
        },
    );
    ctx.cur_block = own_blk;
    if st.is_refcounted() {
        ctx.emit_rc_inc(loaded);
    }
    let Some(slot) = slot else {
        ctx.f.set_term(ctx.cur_block, Terminator::Br(merge));
        ctx.cur_block = merge;
        return (st, loaded);
    };
    let loaded = match ty == st {
        true => loaded,
        false => ctx.box_to_any(loaded),
    };
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(loaded, Operand::Value(slot), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(merge));
    ctx.cur_block = merge;
    let v = ctx
        .f
        .append_inst(merge, InstKind::Load(ty, Operand::Value(slot), 0), ty, None);
    (ty, Operand::Value(v))
}

/// Whether `prev` is a lowered `null` a pointer slot of type `st` holds
/// as it is.
fn null_fits(prev: Operand, st: Type) -> bool {
    matches!(prev, Operand::ConstPtrNull)
        && !matches!(
            st,
            Type::F64 | Type::I64 | Type::I32 | Type::Bool | Type::Any
        )
}

/// Whether the checker typed the field `name` of the literal `lit` as
/// an any-lane value.
fn lit_field_is_any(ctx: &LowerCtx<'_>, lit: ExprId, name: &PropKey) -> bool {
    let Some(CType::Struct(fs)) = ctx.expr_types.get(&lit) else {
        return false;
    };
    fs.iter()
        .find(|(n, _)| n == name)
        .is_some_and(|(_, t)| matches!(t.clone().lane(), CType::Any))
}
