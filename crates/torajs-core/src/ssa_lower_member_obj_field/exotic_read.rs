//! The guard in front of a typed class-instance field load.
//!
//! The load reads the layout slot, which is the field's value exactly
//! as long as nothing redefined the field. `Object.defineProperty` can
//! turn a declared field into an accessor (§10.1.6.3), and then every
//! read is a getter call (§10.1.8.1 step 7). Only a program that names
//! an accessor definer can do that, and with a literal key only to that
//! name (`Ast::accessor_field_names`); every other read keeps the bare
//! load. A read that can meet one stands behind one bit test on the
//! header word: `FLAG_OBJ_EXOTIC_FIELD` is raised by the redefine and by
//! nothing an untouched instance ever goes through, so the instances
//! that were never redefined pay one not-taken branch. A hit runs the
//! any-lane [[Get]]
//! (`__torajs_obj_field_get_exotic`) and brings its answer into the
//! slot's type as a checked cast ([`convert`]).
//!
//! A refcounted field's read answers OWNED on both arms, so the getter's
//! fresh value has a release site; the plain arm takes a share.

use std::collections::HashMap;

use crate::ast::ExprId;
use crate::ssa::{FuncId, IPred, InstKind, Module, Operand, Terminator, Type};
use crate::ssa_lower::{LowerCtx, declare_intrinsic};

mod convert;

/// The read leg: `(cell, name Str) -> owned any`.
const GET_LEG: &str = "__torajs_obj_field_get_exotic";

pub(crate) fn declare(module: &mut Module, fn_table: &mut HashMap<String, FuncId>) {
    declare_intrinsic(
        module,
        fn_table,
        GET_LEG,
        &[Type::Ptr, Type::Str],
        Type::Any,
    );
    convert::declare(module, fn_table);
}

/// Whether a read of the declared field `name` of the instance `obj`
/// (an operand with no expression of its own when `None`), whose slot
/// is `slot_ty`, can meet an accessor, or a value a write of the
/// program parked in the sidecar.
fn applies(ctx: &LowerCtx<'_>, obj: Option<ExprId>, name: &str, slot_ty: Type) -> bool {
    let names = &ctx.ast.accessor_field_names;
    (names.may(name) || names.may_shadow(name, slot_ty == Type::F64))
        && receiver_may_be_exotic(ctx, obj)
}

/// Whether the instance `obj` can carry the exotic-field bit at this
/// read: a constructor's fresh `this` and a generator's own state
/// cannot.
pub(crate) fn receiver_may_be_exotic(ctx: &LowerCtx<'_>, obj: Option<ExprId>) -> bool {
    obj.is_none_or(|obj| {
        !ctx.proofs.fresh_this_recv.contains(&obj)
            && !crate::ssa_lower_assign_member_field::guard::generator_own_slot(ctx, obj)
    })
}

/// Branch on the instance's exotic-field bit: `(hit, plain)`, with the
/// current block terminated.
pub(super) fn branch(
    ctx: &mut LowerCtx<'_>,
    obj_val: Operand,
) -> (crate::ssa::BlockId, crate::ssa::BlockId) {
    let hdr_word = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I32, obj_val, 4),
        Type::I32,
        None,
    );
    let bit = (torajs_rc::FLAG_OBJ_EXOTIC_FIELD as u32) << 16;
    let masked = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::BinOp(
            crate::ssa::BinOp::And,
            Operand::Value(hdr_word),
            Operand::ConstI32(bit as i32),
        ),
        Type::I32,
        None,
    );
    let hit = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Ne, Operand::Value(masked), Operand::ConstI32(0)),
        Type::Bool,
        None,
    );
    let hit_blk = ctx.f.add_block();
    let plain_blk = ctx.f.add_block();
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(hit),
            then_blk: hit_blk,
            else_blk: plain_blk,
        },
    );
    (hit_blk, plain_blk)
}

/// The any-lane [[Get]] of `name` on the instance, owned.
pub(super) fn emit_get(ctx: &mut LowerCtx<'_>, obj_val: Operand, name: &str) -> Operand {
    let name_str = ctx.intern_string_literal(name);
    let leg = ctx.fn_table[GET_LEG];
    let got = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(leg, vec![obj_val, Operand::Value(name_str)]),
        Type::Any,
        None,
    );
    ctx.emit_throw_check(None);
    Operand::Value(got)
}

/// The guarded load of the field `name` (slot at `offset`, type
/// `field_ty`) of the class instance `obj` the member read `eid`
/// performs. `None` when the read keeps its bare load.
pub(crate) fn guarded_load(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    obj: ExprId,
    obj_val: Operand,
    slot: (u64, Type),
    name: &str,
) -> Option<Operand> {
    if super::shadow_read::applies(ctx, Some(obj), name, slot.1)
        && receiver_may_be_exotic(ctx, Some(obj))
    {
        let v = super::shadow_read::boxed_load(ctx, obj_val, slot, name);
        ctx.owned_member_reads.insert(eid);
        return Some(v);
    }
    let v = owned_slot_read(ctx, Some(obj), obj_val, slot, name)?;
    if slot.1.is_refcounted() {
        ctx.owned_member_reads.insert(eid);
    }
    Some(v)
}

/// A whole-object reader's view of one declared field (a spread, a
/// serializer, `Object.values`): the value, its type, and whether it is
/// OWNED (the guarded read) or borrowed out of the slot (the bare
/// load). A field a write of the program can shadow answers as an
/// owned any-lane value (`shadow_read`), so the type is the value's,
/// not always the slot's.
pub(crate) fn slot_value(
    ctx: &mut LowerCtx<'_>,
    obj: Option<ExprId>,
    obj_val: Operand,
    (offset, field_ty): (u64, Type),
    name: &str,
) -> (Operand, Type, bool) {
    if super::shadow_read::applies(ctx, obj, name, field_ty) && receiver_may_be_exotic(ctx, obj) {
        let v = super::shadow_read::boxed_load(ctx, obj_val, (offset, field_ty), name);
        return (v, Type::Any, true);
    }
    if let Some(v) = owned_slot_read(ctx, obj, obj_val, (offset, field_ty), name) {
        return (v, field_ty, true);
    }
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(field_ty, obj_val, offset),
        field_ty,
        None,
    );
    (Operand::Value(v), field_ty, false)
}

/// [`slot_value`] taken as an owned value: a borrowed one takes a
/// share.
pub(crate) fn owned_field_or_any(
    ctx: &mut LowerCtx<'_>,
    obj: Option<ExprId>,
    obj_val: Operand,
    slot: (u64, Type),
    name: &str,
) -> (Operand, Type) {
    let (v, ty, owned) = slot_value(ctx, obj, obj_val, slot, name);
    if !owned && ty.is_refcounted() {
        ctx.emit_rc_inc(v);
    }
    (v, ty)
}

/// Before a call through the callable field `name`: the call reads
/// the field (§13.3.6.1 step 1), so a getter installed there runs, and
/// its answer must be the closure the typed call then invokes from the
/// slot ([`convert`]'s same-cell row).
pub(crate) fn guard_callee_slot(
    ctx: &mut LowerCtx<'_>,
    obj: ExprId,
    obj_val: Operand,
    slot: (u64, Type),
    name: &str,
) {
    if let Some(v) = owned_slot_read(ctx, Some(obj), obj_val, slot, name) {
        ctx.emit_drop_value(v, slot.1);
    }
}

/// The guarded read as an owned value (a refcounted one carries a
/// stake of its own on both arms); `None` when the read keeps its bare
/// load.
fn owned_slot_read(
    ctx: &mut LowerCtx<'_>,
    obj: Option<ExprId>,
    obj_val: Operand,
    (offset, field_ty): (u64, Type),
    name: &str,
) -> Option<Operand> {
    if !applies(ctx, obj, name, field_ty) {
        return None;
    }
    let (conv, shown) = convert::conversion(ctx, obj, name, field_ty)?;
    let res = ctx.alloca_in_entry(field_ty, Some("__field_read"));
    let (hit_blk, plain_blk) = branch(ctx, obj_val);
    let done = ctx.f.add_block();

    ctx.cur_block = hit_blk;
    let got = emit_get(ctx, obj_val, name);
    let v = convert::convert(ctx, &conv, got, field_ty, (obj_val, offset), (name, &shown));
    let blk = ctx.cur_block;
    ctx.f
        .append_void(blk, InstKind::Store(v, Operand::Value(res), 0));
    ctx.f.set_term(blk, Terminator::Br(done));

    ctx.cur_block = plain_blk;
    let v = ctx.f.append_inst(
        plain_blk,
        InstKind::Load(field_ty, obj_val, offset),
        field_ty,
        None,
    );
    if field_ty.is_refcounted() {
        ctx.emit_owned_result_inc(Operand::Value(v), field_ty);
    }
    let blk = ctx.cur_block;
    ctx.f.append_void(
        blk,
        InstKind::Store(Operand::Value(v), Operand::Value(res), 0),
    );
    ctx.f.set_term(blk, Terminator::Br(done));

    ctx.cur_block = done;
    let out = ctx.f.append_inst(
        done,
        InstKind::Load(field_ty, Operand::Value(res), 0),
        field_ty,
        None,
    );
    Some(Operand::Value(out))
}

/// The optional-chain spelling (`o?.f`): the field's value boxed and
/// owned, written to `res_slot` on both arms, which meet at the
/// returned block (`cur_block` is left there). `None` when the read
/// keeps its bare load.
pub(crate) fn guarded_load_boxed(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    obj: ExprId,
    obj_val: Operand,
    slot: (u64, Type),
    name: &str,
    res_slot: crate::ssa::ValueId,
) -> Option<crate::ssa::BlockId> {
    if !applies(ctx, Some(obj), name, slot.1) {
        return None;
    }
    let done = boxed_read(ctx, obj_val, slot, name, res_slot);
    ctx.owned_member_reads.insert(eid);
    Some(done)
}

/// A whole-object reader whose result is `any` anyway (a serializer, a
/// key-value pair): the field's value boxed and owned, the getter's
/// answer as it came, so no cast stands between the two. `None` when
/// the read keeps its bare load.
pub(crate) fn any_field_value(
    ctx: &mut LowerCtx<'_>,
    obj_val: Operand,
    slot: (u64, Type),
    name: &str,
) -> Option<Operand> {
    if !applies(ctx, None, name, slot.1) {
        return None;
    }
    let res = ctx.alloca_in_entry(Type::Any, Some("__field_read"));
    let done = boxed_read(ctx, obj_val, slot, name, res);
    let v = ctx.f.append_inst(
        done,
        InstKind::Load(Type::Any, Operand::Value(res), 0),
        Type::Any,
        None,
    );
    Some(Operand::Value(v))
}

/// Both arms of a boxed guarded read, each writing its owned `any` to
/// `res_slot`; `cur_block` is left at the returned join.
fn boxed_read(
    ctx: &mut LowerCtx<'_>,
    obj_val: Operand,
    (offset, field_ty): (u64, Type),
    name: &str,
    res_slot: crate::ssa::ValueId,
) -> crate::ssa::BlockId {
    let (hit_blk, plain_blk) = branch(ctx, obj_val);
    let done = ctx.f.add_block();

    ctx.cur_block = hit_blk;
    let got = emit_get(ctx, obj_val, name);
    let blk = ctx.cur_block;
    ctx.f
        .append_void(blk, InstKind::Store(got, Operand::Value(res_slot), 0));
    ctx.f.set_term(blk, Terminator::Br(done));

    ctx.cur_block = plain_blk;
    let v = ctx.f.append_inst(
        plain_blk,
        InstKind::Load(field_ty, obj_val, offset),
        field_ty,
        None,
    );
    if field_ty.is_refcounted() {
        ctx.emit_owned_result_inc(Operand::Value(v), field_ty);
    }
    // an f64 slot may hold the undefined sentinel, which boxes as
    // `undefined`, not as a NaN
    let boxed = match field_ty {
        Type::F64 => ctx.box_f64_or_undef(Operand::Value(v)),
        _ => ctx.box_to_any(Operand::Value(v)),
    };
    let blk = ctx.cur_block;
    ctx.f
        .append_void(blk, InstKind::Store(boxed, Operand::Value(res_slot), 0));
    ctx.f.set_term(blk, Terminator::Br(done));

    ctx.cur_block = done;
    done
}
