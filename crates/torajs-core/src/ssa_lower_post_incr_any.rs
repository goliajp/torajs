//! The `any` lanes of [`crate::ssa_lower_post_incr`] — an `any`
//! slot (`x++` on an any local / global) and an any receiver's
//! member (`o.f++`), both one runtime step (ToNumeric, +1 in the
//! value's own numeric domain, store) with the postfix / prefix
//! answer picked by [`Step`]. Split from the parent (rotation 626)
//! when the prefix form joined and pushed it past 500 lines; the
//! keyed any lane (`a[k]++`) was already the sibling
//! `ssa_lower_post_incr_keyed`.

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_post_incr::Step;

/// The `any`-slot lane — one call carrying the whole §13.4.4.1 step
/// (load, ToNumeric, add one in the operand's own numeric domain,
/// release the replaced value, store) and answering the coerced old
/// value. See `Intrinsics::any_incr_slot` for why the runtime takes
/// the slot pointer instead of the loaded value.
pub(crate) fn lower_any_slot(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    slot: Operand,
    step: Step,
) -> Operand {
    let fid = ctx.intrinsics.any_incr_slot;
    let flag = Operand::ConstI64(if step.is_inc { 1 } else { 0 });
    let cur_block = ctx.cur_block;
    let old = ctx.f.append_inst(
        cur_block,
        InstKind::Call(fid, vec![slot, flag]),
        Type::Any,
        None,
    );
    // A throwing ToNumeric answers undefined and stores nothing.
    ctx.emit_throw_check(None);
    if !step.is_prefix {
        return Operand::Value(old);
    }
    // Prefix — the answer is the stepped value the kernel just
    // stored: release the coerced old box, read the slot back, and
    // take the consumer's own stake in it. Owned, not borrowed: a
    // borrowed load dangles once a later update of the same slot
    // releases the cell it points at (`console.log(++b, b, --b)` on
    // a BigInt printed the freed cell as 0n).
    ctx.emit_drop_value(Operand::Value(old), Type::Any);
    let cur_block = ctx.cur_block;
    let new_v = ctx.f.append_inst(
        cur_block,
        InstKind::Load(Type::Any, slot, 0),
        Type::Any,
        None,
    );
    ctx.emit_owned_result_inc(Operand::Value(new_v), Type::Any);
    ctx.owned_member_reads.insert(eid);
    Operand::Value(new_v)
}

/// A typed instance's `any` field (`o.f++` where `f: any`) whose store
/// no guard stands in front of: the slot's value goes through the
/// value kernel, the slot then releases what it holds at the store and
/// takes the stepped value. The slot is read again for that release —
/// a `valueOf` the ToNumeric ran may have stored into it.
pub(crate) fn lower_any_field(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    obj_val: Operand,
    offset: u64,
    step: Step,
) -> Operand {
    let cur = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::Any, obj_val, offset),
        Type::Any,
        None,
    );
    // the kernel borrows `cur` across ToNumeric, and a `valueOf` that
    // stores into this field would otherwise free it mid-call
    ctx.emit_owned_result_inc(Operand::Value(cur), Type::Any);
    let old_slot = ctx.alloca(Type::Any, Some("__incr_old"));
    let flag = Operand::ConstI64(if step.is_inc { 1 } else { 0 });
    let new_v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.anyv_incr_value,
            vec![Operand::Value(cur), flag, Operand::Value(old_slot)],
        ),
        Type::Any,
        None,
    );
    ctx.emit_drop_value(Operand::Value(cur), Type::Any);
    // A throwing ToNumeric leaves both answers undefined: no PutValue.
    ctx.emit_throw_check(None);
    let held = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::Any, obj_val, offset),
        Type::Any,
        None,
    );
    ctx.emit_drop_value(Operand::Value(held), Type::Any);
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Store(Operand::Value(new_v), obj_val, offset),
    );
    ctx.owned_member_reads.insert(eid);
    if step.is_prefix {
        // the slot holds the kernel's stake; the answer takes its own
        let old = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(Type::Any, Operand::Value(old_slot), 0),
            Type::Any,
            None,
        );
        ctx.emit_drop_value(Operand::Value(old), Type::Any);
        ctx.emit_owned_result_inc(Operand::Value(new_v), Type::Any);
        return Operand::Value(new_v);
    }
    let old = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::Any, Operand::Value(old_slot), 0),
        Type::Any,
        None,
    );
    Operand::Value(old)
}

/// The `any`-receiver lane of [`lower_member`] — §13.4.4.1 over a
/// receiver whose shape is a runtime question. Three existing lanes
/// compose: the accessor-aware any-member read answers the current
/// value (GetV), `anyv_incr_value` runs ToNumeric exactly once and
/// steps in the operand's own numeric domain, and the tag-gated
/// member-set tail stores the new value with the full setter /
/// refusal semantics (write-back included for a named Any receiver).
/// The expression answers the coerced OLD value — owned, so the
/// PostIncr eid joins `owned_member_reads` and consumers release it.
// operands of one member update plus the receiver ownership facts
#[allow(clippy::too_many_arguments)]
pub(crate) fn lower_member_any(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    target: ExprId,
    obj_ident: Option<String>,
    recv_owned: bool,
    obj_val: Operand,
    field: &str,
    step: Step,
) -> Operand {
    let old_box = crate::ssa_lower_any_member::lower_any_member_read(ctx, target, obj_val, field);
    let old_slot = ctx.alloca(Type::Any, Some("__incr_old"));
    let flag = Operand::ConstI64(if step.is_inc { 1 } else { 0 });
    let cur_block = ctx.cur_block;
    let new_v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.anyv_incr_value,
            vec![old_box, flag, Operand::Value(old_slot)],
        ),
        Type::Any,
        None,
    );
    ctx.emit_drop_value(old_box, Type::Any);
    // A throwing ToNumeric leaves both answers undefined: no PutValue.
    ctx.emit_throw_check(None);
    // Owned unbox (tag, payload+1) feeds the member-set kernel; the
    // surplus box stake releases after the store — the
    // `lower_dynobj_assign_any_payload` contract.
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
    crate::ssa_lower_assign_member_any::emit_any_member_set(
        ctx,
        obj_val,
        field,
        Operand::Value(tag_v),
        Operand::Value(val_v),
        &obj_ident,
        recv_owned,
    );
    ctx.owned_member_reads.insert(eid);
    if step.is_prefix {
        // Prefix answers the stepped value, owned like the old one
        // is: the set took one stake off `new_v`'s box, the drop
        // below would release the other — keep it for the consumer.
        return Operand::Value(new_v);
    }
    ctx.emit_drop_value(Operand::Value(new_v), Type::Any);
    let cur_block = ctx.cur_block;
    let old = ctx.f.append_inst(
        cur_block,
        InstKind::Load(Type::Any, Operand::Value(old_slot), 0),
        Type::Any,
        None,
    );
    Operand::Value(old)
}
