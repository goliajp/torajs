//! The read of an optional `number` member some write of the program
//! can shadow (`AccessorFieldNames::may_shadow`).
//!
//! The slot holds a value of its type; a write the slot could not take
//! — a `null` into a number slot, a string through an `any` receiver —
//! lives in the field's sidecar behind `FLAG_OBJ_EXOTIC_FIELD`, and
//! §10.1.8.1 [[Get]] answers it. No value of the slot's type can carry
//! what the sidecar may hold, so the read answers an owned any-lane
//! value: the sidecar's value as it is on a hit, the slot's value boxed
//! on the plain arm. The consumers of the read take the any lane from
//! there, which is what a slot the layout typed `any` always gave them;
//! a program whose writes cannot shadow the field never comes here.

use crate::ast::ExprId;
use crate::ast::named_prop_census::shadow_names::SlotKind;
use crate::ssa::{BlockId, IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::LowerCtx;

/// Whether the read of the field `name` of the receiver `obj`, whose
/// slot is `slot_ty`, answers the sidecar as an any-lane value: an
/// optional `number` member's f64 slot a write of the program can
/// shadow. Its checked type, `number | undefined`, already sends its
/// consumers down the any lane when the value arrives boxed; a member
/// of another type keeps the cast the accessor read runs
/// (`super::exotic_read::convert`), which refuses a value of another
/// type aloud. The receiver's type says whether the member is optional
/// — a same-named required `number` member of another type is read as
/// the f64 its consumers expect (`optional_number::optional_number_slot`).
pub(crate) fn applies(ctx: &LowerCtx<'_>, obj: Option<ExprId>, name: &str, slot_ty: Type) -> bool {
    slot_ty == Type::F64
        && ctx.ast.accessor_field_names.may_shadow(name, true)
        && crate::ssa_lower_object_lit::optional_number::optional_number_slot(
            ctx, obj, name, slot_ty,
        )
}

/// Whether the member read `eid` of the field `name`, whose slot is
/// `slot_ty`, is one the checker typed `any` because an any-lane write
/// of the program can leave the field holding a value of another type
/// (`check_type_of_member::mistyped_read`): the read answers the
/// sidecar's value as it is, and its consumers take the any lane. The
/// checker's verdict decides, so a scalar slot whose name a write of
/// either kind reaches follows it; boxing the slot is right whatever
/// the slot holds.
pub(crate) fn mistyped(ctx: &LowerCtx<'_>, eid: ExprId, name: &str, slot_ty: Type) -> bool {
    matches!(ctx.expr_types.get(&eid), Some(crate::check::Type::Any))
        && scalar_slot(slot_ty)
        && (ctx.ast.accessor_field_names).mistyped_read(name, SlotKind::Other)
}

/// A slot of a member the checker can retype: a number, a string, a
/// boolean.
fn scalar_slot(slot_ty: Type) -> bool {
    matches!(
        slot_ty,
        Type::F64 | Type::I64 | Type::I32 | Type::Bool | Type::Str | Type::Substr
    )
}

/// Whether a whole-object reader (a spread, `Object.values`) of the
/// field `name`, whose slot is `slot_ty`, reads it as the checker reads
/// the member: an `any` value, since the field may hold a value of
/// another type.
pub(crate) fn mistyped_field(ctx: &LowerCtx<'_>, name: &str, slot_ty: Type) -> bool {
    // the checker retypes a scalar member only (a number, a string, a
    // boolean); a heap member keeps its checked read. The slot's kind is
    // what `SlotKind::of_member` names on the checker's side.
    scalar_slot(slot_ty)
        && (ctx.ast.accessor_field_names).mistyped_read(name, SlotKind::of_slot(slot_ty))
}

/// The slot's value boxed as an owned `any`, for a receiver that
/// cannot carry the sidecar (a constructor's fresh `this`).
pub(super) fn boxed_slot(
    ctx: &mut LowerCtx<'_>,
    obj_val: Operand,
    (offset, field_ty): (u64, Type),
) -> Operand {
    let blk = ctx.cur_block;
    let v = ctx.f.append_inst(
        blk,
        InstKind::Load(field_ty, obj_val, offset),
        field_ty,
        None,
    );
    box_owned(ctx, Operand::Value(v), field_ty)
}

/// A slot value boxed as an owned `any` (an f64 slot's undefined
/// sentinel boxes as `undefined`).
fn box_owned(ctx: &mut LowerCtx<'_>, v: Operand, field_ty: Type) -> Operand {
    match field_ty {
        Type::F64 => ctx.box_f64_or_undef(v),
        _ if field_ty.is_refcounted() => {
            ctx.emit_owned_result_inc(v, field_ty);
            // a heap slot spells `undefined` with a cell of its own
            match crate::ssa_lower_binop_null_undef::undef_cell_sym(field_ty) {
                Some(_) => ctx.box_heap_slot_or_undef(v),
                None => ctx.box_to_any(v),
            }
        }
        _ => ctx.box_to_any(v),
    }
}

/// The field's value as an owned `any`: the sidecar's on a hit, the
/// slot's boxed otherwise (an f64 slot's undefined sentinel boxes as
/// `undefined`). The read `eid` on the left of a `??` that asked for it
/// (`ssa_lower_nullish_any::split`) answers the nullish test on each
/// arm instead: a nullish value branches to a block left open for the
/// `??`, so the plain arm boxes only a number and the tag is read only
/// on a hit.
pub(super) fn boxed_load(
    ctx: &mut LowerCtx<'_>,
    eid: Option<ExprId>,
    obj_val: Operand,
    (offset, field_ty): (u64, Type),
    name: &str,
) -> Operand {
    let split =
        field_ty == Type::F64 && eid.is_some_and(|e| ctx.binop.nullish_split == Some((e, None)));
    let res = ctx.alloca_in_entry(Type::Any, Some("__field_read"));
    let (hit_blk, plain_blk) = super::exotic_read::branch(ctx, obj_val);
    let done = ctx.f.add_block();
    let nullish = split.then(|| ctx.f.add_block());

    ctx.cur_block = hit_blk;
    let got = super::exotic_read::emit_get(ctx, obj_val, name);
    let blk = ctx.cur_block;
    ctx.f
        .append_void(blk, InstKind::Store(got, Operand::Value(res), 0));
    let term = match nullish {
        Some(n) => {
            let tag = ctx.f.append_inst(
                blk,
                InstKind::Call(ctx.intrinsics.any_unbox_tag, vec![got]),
                Type::I64,
                None,
            );
            cond_br(crate::ssa_lower_nullish_any::nullish_tag(ctx, tag), n, done)
        }
        None => Terminator::Br(done),
    };
    ctx.f.set_term(blk, term);

    ctx.cur_block = plain_blk;
    let v = ctx.f.append_inst(
        plain_blk,
        InstKind::Load(field_ty, obj_val, offset),
        field_ty,
        None,
    );
    let boxed = match nullish {
        Some(n) => box_number_or_branch(ctx, Operand::Value(v), n),
        None => box_owned(ctx, Operand::Value(v), field_ty),
    };
    let blk = ctx.cur_block;
    ctx.f
        .append_void(blk, InstKind::Store(boxed, Operand::Value(res), 0));
    ctx.f.set_term(blk, Terminator::Br(done));
    if let (Some(e), Some(n)) = (eid, nullish) {
        ctx.binop.nullish_split = Some((e, Some(n)));
    }

    ctx.cur_block = done;
    let out = ctx.f.append_inst(
        done,
        InstKind::Load(Type::Any, Operand::Value(res), 0),
        Type::Any,
        None,
    );
    Operand::Value(out)
}

/// The f64 slot value `v` boxed as a number; its undefined sentinel
/// branches to `nullish` instead.
fn box_number_or_branch(ctx: &mut LowerCtx<'_>, v: Operand, nullish: BlockId) -> Operand {
    let bits = ctx
        .f
        .append_inst(ctx.cur_block, InstKind::BitCastF64ToI64(v), Type::I64, None);
    let sentinel = crate::ssa_lower_undef_f64_source::F64_UNDEF_SENTINEL_BITS as i64;
    let is_undef = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Eq, Operand::Value(bits), Operand::ConstI64(sentinel)),
        Type::Bool,
        None,
    );
    let num = ctx.f.add_block();
    ctx.f
        .set_term(ctx.cur_block, cond_br(is_undef, nullish, num));
    ctx.cur_block = num;
    let boxed = ctx.f.append_inst(
        num,
        InstKind::BoxAny(3, Operand::Value(bits)),
        Type::Any,
        None,
    );
    Operand::Value(boxed)
}

fn cond_br(cond: ValueId, then_blk: BlockId, else_blk: BlockId) -> Terminator {
    Terminator::CondBr {
        cond: Operand::Value(cond),
        then_blk,
        else_blk,
    }
}
