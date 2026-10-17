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
        let mut deletable = false;
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
                let key = PropKey::from(prop);
                if crate::ssa_lower_member_obj_field::deleted_accessor::reaches(ctx, &key.lossy()) {
                    let fields = (&mut *field_tys, &mut *field_vals);
                    let src = (eid, src_op, before);
                    let v = deletable_getter(ctx, src, (&key, off, sig_id), fields, absent);
                    // the member may be absent whatever the source's type says
                    deletable = true;
                    (key, Type::Any, v)
                } else {
                    let ret = ctx.fn_sigs[sig_id.0 as usize].1;
                    // Owned already — the getter returns its own value, so no
                    // rc_inc (unlike the borrowed field load below).
                    let lane =
                        crate::ssa_lower_call_struct_method_dispatch::objlit_slot::emit_getter_call;
                    let v = lane(ctx, Some(eid), src_op, off, sig_id);
                    (key, ret, v)
                }
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
                    let read = crate::ssa_lower_member_obj_field::exotic_read::owned_field_or_any;
                    let (v, ty) = read(ctx, Some(eid), src_op, (off, *st), &name);
                    (sn.clone(), ty, v)
                }
            }
        };
        if !deletable && !may_be_absent(ctx, eid, &name) {
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
    // a field a write of the program can shadow reads as an any-lane
    // value, so the member takes the any lane whatever came before
    use crate::ssa_lower_member_obj_field::shadow_read;
    let shadowed = (shadow_read::applies(ctx, Some(src_eid), &name.lossy(), st)
        || shadow_read::mistyped_field(ctx, &name.lossy(), st))
        && crate::ssa_lower_member_obj_field::exotic_read::receiver_may_be_exotic(
            ctx,
            Some(src_eid),
        );
    let prev = field_tys.iter().position(|(k, _)| k == name);
    let (ty, prev_val) = match prev {
        None if shadowed => (Type::Any, None),
        None => (st, None),
        Some(pos) if shadowed && field_tys[pos].1 == Type::Any => {
            (Type::Any, Some((field_vals[pos], Type::Any)))
        }
        // a `null` the slot spells as its own null pointer
        Some(pos) if !shadowed && (field_tys[pos].1 == st || null_fits(field_vals[pos], st)) => {
            (st, Some((field_vals[pos], st)))
        }
        Some(pos) => {
            let boxed = prev_as_any(ctx, before, name, (field_tys[pos].1, field_vals[pos]));
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
            let raw = Operand::Value(raw);
            match (ty, st) {
                (Type::Any, Type::F64) => ctx.box_f64_or_undef(raw),
                (Type::Any, _) => ctx.box_to_any(raw),
                _ => raw,
            }
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
    let read = crate::ssa_lower_member_obj_field::exotic_read::owned_field_or_any;
    let (owned, got) = read(ctx, Some(src_eid), src, (off, st), &name.lossy());
    let owned = match (ty == got, st) {
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

/// The earlier member `name`'s value `v` of slot type `ty`, boxed to
/// the owned `any` a member that takes the any lane keeps.
fn prev_as_any(
    ctx: &mut LowerCtx<'_>,
    before: &[(PropKey, ExprId)],
    name: &PropKey,
    (ty, v): (Type, Operand),
) -> Operand {
    // an `undefined` the literal spelled lowers to the null pointer
    let undef = before
        .iter()
        .rev()
        .find(|(k, _)| k == name)
        .is_some_and(|(_, e)| matches!(ctx.expr_types.get(e), Some(CType::Undefined)));
    match ty {
        Type::Any => v,
        _ if undef && v == Operand::ConstPtrNull => {
            crate::ssa_lower_object_lit::absent::any_undef(ctx)
        }
        // an optional `number` member's f64 may be its sentinel
        Type::F64 if ctx.undefable_f64_fields.contains(name) => ctx.box_f64_or_undef(v),
        _ => ctx.box_to_any(v),
    }
}

/// The contribution of the source accessor `name` (getter slot at
/// `off`, signature `sig`) that a delete of the program can reach, as
/// an owned `any`. §7.3.25 CopyDataProperties copies only the keys the
/// source owns: a deleted member contributes nothing, so the earlier
/// member's value stays (or the key stays absent, recorded in `absent`
/// as for an optional member); a data property a store after the delete
/// created contributes its value; a live accessor its getter's answer.
fn deletable_getter(
    ctx: &mut LowerCtx<'_>,
    (src_eid, src, before): (ExprId, Operand, &[(PropKey, ExprId)]),
    (name, off, sig): (&PropKey, u64, crate::ssa::SigId),
    (field_tys, field_vals): (&mut Vec<(PropKey, Type)>, &mut Vec<Operand>),
    absent: &mut Vec<(PropKey, Operand)>,
) -> Operand {
    use crate::ssa_lower_member_obj_field::deleted_accessor as del;
    let prev = field_tys.iter().position(|(k, _)| k == name);
    let prev = prev.map(|pos| prev_as_any(ctx, before, name, (field_tys[pos].1, field_vals[pos])));
    let kept = prev.unwrap_or_else(|| crate::ssa_lower_object_lit::absent::any_undef(ctx));
    let slot = ctx.alloca_in_entry(Type::Any, Some("__spread_field"));
    let lacks_slot = ctx.alloca_in_entry(Type::I64, Some("__spread_lacks"));
    let blk = ctx.cur_block;
    ctx.f
        .append_void(blk, InstKind::Store(kept, Operand::Value(slot), 0));
    let zero = Operand::ConstI64(0);
    ctx.f
        .append_void(blk, InstKind::Store(zero, Operand::Value(lacks_slot), 0));
    let gone = del::branch_gone(ctx, src, &name.lossy());
    let deleted_blk = ctx.f.add_block();
    let replaced_blk = ctx.f.add_block();
    let done = ctx.f.add_block();
    let is_deleted = ctx.f.append_inst(
        gone.gone_blk,
        InstKind::ICmp(IPred::Eq, gone.state, Operand::ConstI64(del::STATE_DELETED)),
        Type::Bool,
        None,
    );
    ctx.f.set_term(
        gone.gone_blk,
        Terminator::CondBr {
            cond: Operand::Value(is_deleted),
            then_blk: deleted_blk,
            else_blk: replaced_blk,
        },
    );
    let one = Operand::ConstI64(1);
    ctx.f.append_void(
        deleted_blk,
        InstKind::Store(one, Operand::Value(lacks_slot), 0),
    );
    ctx.f.set_term(deleted_blk, Terminator::Br(done));

    ctx.cur_block = replaced_blk;
    if let Some(v) = prev {
        ctx.emit_drop_value(v, Type::Any);
    }
    let v = del::own_value(ctx, src, gone.name_str);
    let blk = ctx.cur_block;
    ctx.f
        .append_void(blk, InstKind::Store(v, Operand::Value(slot), 0));
    ctx.f.set_term(blk, Terminator::Br(done));

    ctx.cur_block = gone.own_blk;
    if let Some(v) = prev {
        ctx.emit_drop_value(v, Type::Any);
    }
    // the getter's answer is owned
    let lane = crate::ssa_lower_call_struct_method_dispatch::objlit_slot::emit_getter_call;
    let v = lane(ctx, Some(src_eid), src, off, sig);
    let v = del::box_getter_answer(ctx, v);
    let blk = ctx.cur_block;
    ctx.f
        .append_void(blk, InstKind::Store(v, Operand::Value(slot), 0));
    ctx.f.set_term(blk, Terminator::Br(done));

    ctx.cur_block = done;
    let lacks = ctx.f.append_inst(
        done,
        InstKind::Load(Type::I64, Operand::Value(lacks_slot), 0),
        Type::I64,
        None,
    );
    match prev {
        None => absent.push((name.clone(), Operand::Value(lacks))),
        Some(_) => {
            if let Some(entry) = absent.iter_mut().find(|(n, _)| n == name) {
                let both = ctx.f.append_inst(
                    done,
                    InstKind::BinOp(BinOp::And, entry.1, Operand::Value(lacks)),
                    Type::I64,
                    None,
                );
                entry.1 = Operand::Value(both);
            }
        }
    }
    let v = ctx.f.append_inst(
        done,
        InstKind::Load(Type::Any, Operand::Value(slot), 0),
        Type::Any,
        None,
    );
    Operand::Value(v)
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
