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
    step: crate::ssa_lower_post_incr::Step,
) -> Operand {
    let k_raw = crate::ssa_lower_index_any_key::lower_any_key(ctx, index);
    // `lower_any_key` boxes only the key shapes the checker says need
    // it — a Number key comes back as the bare I64 / F64 it was — and
    // `anyv_to_property_key` reads an AnyValue: handed the integer 0
    // it saw the null box and answered the Str `"null"`, so
    // `o[0]++` read and wrote `o["null"]` (5 stayed 5, a `null` key
    // appeared, an array receiver went on to SIGSEGV). Rotation 622
    // registered that as "the keyed any lane does not increment a
    // Map's expando" — it was every receiver, and it was the key. A
    // non-`any` key is boxed and handed to the keyed kernels as is:
    // they route an integral number to the index lane themselves and
    // ToString the rest, which is ToPropertyKey exactly once. Only an
    // `any` key runs the explicit coercion, whose ToString may throw.
    let k = if matches!(ctx.operand_ty(&k_raw), Type::Any) {
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
        ctx.f.append_inst(
            cur_block,
            InstKind::BoxAny(4, Operand::Value(kp)),
            Type::Any,
            None,
        )
    } else {
        match ctx.box_to_any(k_raw) {
            Operand::Value(v) => v,
            _ => unreachable!("box_to_any answers a value"),
        }
    };
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
    let flag = Operand::ConstI64(if step.is_inc { 1 } else { 0 });
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
    // A throwing ToNumeric leaves both answers undefined: no PutValue.
    ctx.emit_throw_check(None);
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
    let (strict, soft) = (
        ctx.intrinsics.any_index_set_keyed,
        ctx.intrinsics.any_index_set_keyed_soft,
    );
    crate::ssa_lower_set_flavor::emit_any_set(
        ctx,
        strict,
        soft,
        vec![
            recv,
            Operand::Value(k),
            Operand::Value(tag_v),
            Operand::Value(val_v),
            Operand::ConstPtrNull,
        ],
    );
    ctx.emit_throw_check(None);
    // The coerced cell carries the to_property_key +1 (its box is a
    // pure encode); a boxed number carries nothing — either way the
    // drop is the balanced release. Then the surplus new-value box.
    ctx.emit_drop_value(Operand::Value(k), Type::Any);
    ctx.owned_member_reads.insert(eid);
    if step.is_prefix {
        // Prefix answers the stepped value with the stake the drop
        // below would have released (see `lower_member_any`).
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
