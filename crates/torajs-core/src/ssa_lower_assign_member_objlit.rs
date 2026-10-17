//! RFC 20260714-objlit-accessor blade 2 — the object-literal setter
//! write lane, split out of `ssa_lower_assign_member` (which the arm
//! pushed past the 500-line file limit).
//!
//! The getter read lane is its mirror, in `ssa_lower_member_obj_field`.

use crate::ast::{Expr, ExprId};
use crate::ssa::{BlockId, InstKind, Operand, StructId, Terminator, Type};
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
    eid: ExprId,
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
    // §13.15.2 step 1.e / §6.2.5.6 — the right-hand side is evaluated
    // ONCE, before PutValue reaches [[Set]] and reads the setter out of
    // the slot. The same operand is the setter's argument and the
    // assignment's value (not whatever the setter body returned).
    let op = ctx.lower_expr(value);
    let recv = match ctx.ast.get_expr(eid) {
        Expr::Assign { target, .. } => match ctx.ast.get_expr(*target) {
            Expr::Member { obj, .. } => Some(*obj),
            _ => None,
        },
        _ => None,
    };
    let lane = crate::ssa_lower_call_struct_method_dispatch::objlit_slot::emit_setter_call;
    let deleted = crate::ssa_lower_member_obj_field::deleted_accessor::reaches(ctx, field);
    let done = deleted.then(|| store_if_gone(ctx, obj_val, field, op));
    lane(ctx, recv, obj_val, (offset, sig_id), (op, value));
    if let Some(done) = done {
        let blk = ctx.cur_block;
        ctx.f.set_term(blk, Terminator::Br(done));
        ctx.cur_block = done;
    }
    // The setter borrowed the value; the assignment's consumer gets the
    // owned stake (a fresh rhs transfers its own, a borrowed one is inc'd).
    let transfers = ctx.expr_transfers_ownership(value);
    let ty = ctx.operand_ty(&op);
    Some(crate::ssa_lower_assign_member::finish_assign_value(
        ctx, eid, op, ty, transfers,
    ))
}

/// The store of `op` over the accessor member `field` of `obj_val`
/// once a delete removed it: §10.1.9.2 [[Set]] finds no own accessor,
/// so OrdinarySetWithOwnDescriptor creates (or writes) an ordinary
/// data property instead of running the old setter. The any-lane leg
/// takes its own stake in the payload, as the setter borrows it.
/// Leaves `cur_block` where the member is still the accessor and
/// answers the join both arms reach.
fn store_if_gone(ctx: &mut LowerCtx<'_>, obj_val: Operand, field: &str, op: Operand) -> BlockId {
    let gone =
        crate::ssa_lower_member_obj_field::deleted_accessor::branch_gone(ctx, obj_val, field);
    let done = ctx.f.add_block();
    ctx.cur_block = gone.gone_blk;
    let (tag, val) = match (ctx.operand_ty(&op), op) {
        (Type::Ptr, Operand::Value(_)) => (Operand::ConstI64(4), op),
        _ => ctx.box_to_tag_value(op),
    };
    let strict = !crate::ssa_lower_set_flavor::cur_fn_is_sloppy(ctx);
    let args = vec![
        obj_val,
        gone.name_str,
        tag,
        val,
        Operand::ConstI64(i64::from(strict)),
    ];
    let leg = ctx.intrinsics.obj_field_set_exotic;
    ctx.f
        .append_inst(ctx.cur_block, InstKind::Call(leg, args), Type::I64, None);
    ctx.emit_throw_check(None);
    let blk = ctx.cur_block;
    ctx.f.set_term(blk, Terminator::Br(done));
    ctx.cur_block = gone.own_blk;
    done
}
