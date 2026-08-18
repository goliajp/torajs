//! The keyed `any`-receiver update lane, split out of
//! [`crate::ssa_lower_post_incr`] when it pushed that file past the
//! 500-line cap. Stepping `recv[key]` through the keyed kernels is a
//! separate question from the typed-slot lanes that stayed behind.

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// The keyed `any`-receiver lane of `ssa_lower_post_incr::lower_index` — §13.4.4.1 over
/// `recv[key]` where no static lane fits. The key runs ToPropertyKey
/// exactly ONCE (§6.2.5 GetValue writes the coerced key back into
/// the Reference Record) and the coerced cell feeds both the keyed
/// read and the keyed store. `anyv_incr_value` runs ToNumeric once
/// and steps in the operand's own numeric domain (the
/// `ssa_lower_post_incr`'s member-any lane contract); the expression answers the
/// coerced OLD value, owned.
pub(crate) fn lower_index_any_keyed(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    index: ExprId,
    recv: Operand,
    is_inc: bool,
) -> Operand {
    let k_raw = crate::ssa_lower_index_any_key::lower_any_key(ctx, index);
    let key_transfers = ctx.expr_transfers_ownership(index);
    let cur_block = ctx.cur_block;
    let kp = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.anyv_to_property_key, vec![k_raw.clone()]),
        Type::Ptr,
        None,
    );
    // NULL answer = the key's ToString threw; propagate.
    ctx.emit_throw_check(None);
    if key_transfers {
        ctx.emit_drop_value(k_raw, Type::Any);
    }
    let cur_block = ctx.cur_block;
    let k = ctx.f.append_inst(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.any_box,
            vec![Operand::ConstI64(4), Operand::Value(kp)],
        ),
        Type::Any,
        None,
    );
    let cur_block = ctx.cur_block;
    let old_box = ctx.f.append_inst(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.any_index_get_keyed,
            vec![recv.clone(), Operand::Value(k)],
        ),
        Type::Any,
        None,
    );
    ctx.emit_throw_check(None);
    let old_slot = ctx.alloca(Type::Any, Some("__incr_old"));
    let flag = Operand::ConstI64(if is_inc { 1 } else { 0 });
    let cur_block = ctx.cur_block;
    let new_v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.anyv_incr_value,
            vec![Operand::Value(old_box), flag, Operand::Value(old_slot)],
        ),
        Type::Any,
        None,
    );
    ctx.emit_drop_value(Operand::Value(old_box), Type::Any);
    // Owned unbox (tag, payload+1) transfers into the keyed store —
    // the `lower_member_any` contract; the surplus box stake releases
    // after it.
    let cur_block = ctx.cur_block;
    let tag_v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.any_unbox_tag, vec![Operand::Value(new_v)]),
        Type::I64,
        None,
    );
    let val_v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.any_unbox_value_owned,
            vec![Operand::Value(new_v)],
        ),
        Type::I64,
        None,
    );
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.any_index_set_keyed,
            vec![
                recv,
                Operand::Value(k),
                Operand::Value(tag_v),
                Operand::Value(val_v),
                Operand::ConstPtrNull,
            ],
        ),
    );
    ctx.emit_throw_check(None);
    // The coerced cell carries the to_property_key +1 (its box is a
    // pure encode) — release it, then the surplus new-value box.
    ctx.emit_drop_value(Operand::Value(k), Type::Any);
    ctx.emit_drop_value(Operand::Value(new_v), Type::Any);
    ctx.owned_member_reads.insert(eid);
    let cur_block = ctx.cur_block;
    let old = ctx.f.append_inst(
        cur_block,
        InstKind::Load(Type::Any, Operand::Value(old_slot), 0),
        Type::Any,
        None,
    );
    Operand::Value(old)
}
