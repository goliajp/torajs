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

use crate::ast::{ExprId, PropKey};
use crate::check::Type as CType;
use crate::ssa::{BinOp, IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::{LowerCtx, OBJ_HEADER_SIZE};

/// `omit` — the destructuring-rest desugar's excluded keys
/// (chunk 707, decoded from the `__spread_omit__:` sentinel);
/// omitted fields are neither loaded nor rc-inc'd. Answers the
/// source cell and its layout.
///
/// A field the source's type declares optional may be absent from the
/// source cell (`ssa_lower_object_lit::absent`), and CopyDataProperties
/// copies only the keys the source owns: such a field contributes its
/// value only when present, and `absent` collects, per name, whether
/// every contributor so far lacked it (I64 1 / 0). A field the type
/// declares required is always there. `before` is the members the
/// literal spells ahead of the spread.
pub(crate) fn unfold_spread(
    ctx: &mut LowerCtx<'_>,
    (eid, before): (ExprId, &[(PropKey, ExprId)]),
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
                if may_be_absent(ctx, eid, sn) {
                    let fields = (&mut *field_tys, &mut *field_vals);
                    let src = (eid, src_op, before);
                    let (ty, val) = maybe_absent(ctx, src, (sn, *st, off), fields, absent);
                    (sn.clone(), ty, val)
                } else {
                    let name = sn.lossy();
                    let read = crate::ssa_lower_member_obj_field::exotic_read::owned_field;
                    let v = read(ctx, Some(eid), src_op, (off, *st), &name);
                    (sn.clone(), *st, v)
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
/// type declares it optional. The checker asks the same question when
/// it types the member (`check_type_of_object_lit`).
fn may_be_absent(ctx: &LowerCtx<'_>, src: ExprId, name: &PropKey) -> bool {
    matches!(ctx.expr_types.get(&src), Some(CType::Struct(fs)) if fs.is_optional(name))
}

/// The contribution of a source field that may be absent: the source's
/// value when it owns the field, else whatever an earlier member put
/// there, whose stake the source's value releases. When the two slot
/// types differ the field takes the any lane, which the checker gave it
/// too (a union of the two), and both values are boxed.
fn maybe_absent(
    ctx: &mut LowerCtx<'_>,
    (src_eid, src, before): (ExprId, Operand, &[(PropKey, ExprId)]),
    (name, st, off): (&PropKey, Type, u64),
    (field_tys, field_vals): (&mut Vec<(PropKey, Type)>, &mut Vec<Operand>),
    absent: &mut Vec<(PropKey, Operand)>,
) -> (Type, Operand) {
    let prev = field_tys.iter().position(|(k, _)| k == name);
    let (ty, prev_val) = match prev {
        None => (st, None),
        // a `null` the slot spells as its own null pointer
        Some(pos) if field_tys[pos].1 == st || null_fits(field_vals[pos], st) => {
            (st, Some((field_vals[pos], st)))
        }
        Some(pos) => {
            // an `undefined` the literal spelled lowers to the null pointer
            let undef = before
                .iter()
                .rev()
                .find(|(k, _)| k == name)
                .is_some_and(|(_, e)| matches!(ctx.expr_types.get(e), Some(CType::Undefined)));
            let boxed = match field_tys[pos].1 {
                Type::Any => field_vals[pos],
                _ if undef && field_vals[pos] == Operand::ConstPtrNull => {
                    crate::ssa_lower_object_lit::absent::any_undef(ctx)
                }
                // an optional `number` member's f64 may be its sentinel
                Type::F64 if ctx.undefable_f64_fields.contains(name) => {
                    ctx.box_f64_or_undef(field_vals[pos])
                }
                _ => ctx.box_to_any(field_vals[pos]),
            };
            (Type::Any, Some((boxed, Type::Any)))
        }
    };
    let lacks = crate::ssa_lower_object_lit::absent::field_absent(ctx, src, name);
    match prev_val {
        None => absent.push((name.clone(), lacks)),
        Some(_) => {
            if let Some(slot) = absent.iter_mut().find(|(n, _)| n == name) {
                let both = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::BinOp(BinOp::And, slot.1, lacks),
                    Type::I64,
                    None,
                );
                slot.1 = Operand::Value(both);
            }
        }
    }
    // an absent field keeps the earlier value, or the source slot's
    // undefined when there is none
    let kept = match prev_val {
        Some((v, _)) => v,
        None => {
            let raw = ctx
                .f
                .append_inst(ctx.cur_block, InstKind::Load(st, src, off), st, None);
            Operand::Value(raw)
        }
    };
    let slot = ctx.alloca_in_entry(ty, Some("__spread_field"));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(kept, Operand::Value(slot), 0),
    );
    let is_absent = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Ne, lacks, Operand::ConstI64(0)),
        Type::Bool,
        None,
    );
    let own_blk = ctx.f.add_block();
    let merge = ctx.f.add_block();
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(is_absent),
            then_blk: merge,
            else_blk: own_blk,
        },
    );
    ctx.cur_block = own_blk;
    if let Some((v, v_ty)) = prev_val {
        ctx.emit_drop_value(v, v_ty);
    }
    let read = crate::ssa_lower_member_obj_field::exotic_read::owned_field;
    let owned = read(ctx, Some(src_eid), src, (off, st), &name.lossy());
    let owned = match (ty == st, st) {
        (true, _) => owned,
        (false, Type::F64) if ctx.undefable_f64_fields.contains(name) => {
            ctx.box_f64_or_undef(owned)
        }
        (false, _) => ctx.box_to_any(owned),
    };
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(owned, Operand::Value(slot), 0),
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
