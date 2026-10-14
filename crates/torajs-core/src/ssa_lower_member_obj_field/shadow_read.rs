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
use crate::ssa::{InstKind, Operand, Terminator, Type};
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
/// the f64 its consumers expect (`absent::optional_number_slot`).
pub(crate) fn applies(ctx: &LowerCtx<'_>, obj: Option<ExprId>, name: &str, slot_ty: Type) -> bool {
    slot_ty == Type::F64
        && ctx.ast.accessor_field_names.may_shadow(name, true)
        && crate::ssa_lower_object_lit::absent::optional_number_slot(ctx, obj, name, slot_ty)
}

/// The field's value as an owned `any`: the sidecar's on a hit, the
/// slot's boxed otherwise (an f64 slot's undefined sentinel boxes as
/// `undefined`).
pub(super) fn boxed_load(
    ctx: &mut LowerCtx<'_>,
    obj_val: Operand,
    (offset, field_ty): (u64, Type),
    name: &str,
) -> Operand {
    let res = ctx.alloca_in_entry(Type::Any, Some("__field_read"));
    let (hit_blk, plain_blk) = super::exotic_read::branch(ctx, obj_val);
    let done = ctx.f.add_block();

    ctx.cur_block = hit_blk;
    let got = super::exotic_read::emit_get(ctx, obj_val, name);
    let blk = ctx.cur_block;
    ctx.f
        .append_void(blk, InstKind::Store(got, Operand::Value(res), 0));
    ctx.f.set_term(blk, Terminator::Br(done));

    ctx.cur_block = plain_blk;
    let v = ctx.f.append_inst(
        plain_blk,
        InstKind::Load(field_ty, obj_val, offset),
        field_ty,
        None,
    );
    let v = Operand::Value(v);
    let boxed = match field_ty {
        Type::F64 => ctx.box_f64_or_undef(v),
        _ => {
            if field_ty.is_refcounted() {
                ctx.emit_owned_result_inc(v, field_ty);
            }
            ctx.box_to_any(v)
        }
    };
    let blk = ctx.cur_block;
    ctx.f
        .append_void(blk, InstKind::Store(boxed, Operand::Value(res), 0));
    ctx.f.set_term(blk, Terminator::Br(done));

    ctx.cur_block = done;
    let out = ctx.f.append_inst(
        done,
        InstKind::Load(Type::Any, Operand::Value(res), 0),
        Type::Any,
        None,
    );
    Operand::Value(out)
}
