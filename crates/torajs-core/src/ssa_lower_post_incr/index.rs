//! `xs[i]++` / `++xs[i]`: the step of an array element. Moved out of
//! [`super`].

use super::Step;
use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_post_incr_len::incr_op_one;

pub(super) fn lower_index(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    obj: ExprId,
    index: ExprId,
    step: Step,
) -> Operand {
    let is_non_deque = ctx.arr_expr_is_non_deque(obj);
    let arr_val = ctx.lower_expr(obj);
    let arr_ty = ctx.operand_ty(&arr_val);
    // Cluster #4 blade 2 — an `any` (or boxed-struct) receiver's
    // keyed update rides the keyed kernels, mirroring
    // [`lower_member_any`]. A null/undefined receiver raises inside
    // the keyed read.
    if matches!(arr_ty, Type::Any | Type::Obj(_)) {
        let recv = if matches!(arr_ty, Type::Obj(_)) {
            ctx.box_to_any(arr_val)
        } else {
            arr_val
        };
        return crate::ssa_lower_post_incr_keyed::lower_index_any_keyed(
            ctx, eid, index, recv, step,
        );
    }
    let elem_ty = match arr_ty {
        Type::Arr(arr_id) => ctx.arr_layouts[arr_id.0 as usize],
        // A PRIMITIVE receiver: `n[0]++` is §13.4.4.1 over a value
        // with no properties, so it raises the TypeError §10.1.8.1
        // owes a strict-mode write — which is exactly what the keyed
        // any lane above already answers. Only the typed spelling was
        // refused, and only by the panic below: a specialized body's
        // `n[0]++` said "not yet supported: post-incr index on
        // non-array I64" while `const n: any = 1; n[0]++` threw.
        //
        // The other shapes this arm catches (Map, Set, Date, RegExp …)
        // deliberately keep the loud refusal. Asked one at a time
        // rather than assumed from the member ladder's list: the keyed
        // any lane does NOT increment a Map's expando —
        // `mm[0] = 5; mm[0]++` leaves it at 5 where bun answers 6 —
        // so boxing them here would trade a compile-time refusal for
        // a silent wrong answer, which is the trade rotation 608
        // declined on the read side. They are registered against that
        // lane instead.
        Type::I64 | Type::I32 | Type::F64 | Type::Bool | Type::Str | Type::Substr => {
            let boxed = ctx.box_to_any(arr_val);
            return crate::ssa_lower_post_incr_keyed::lower_index_any_keyed(
                ctx, eid, index, boxed, step,
            );
        }
        other => panic!("ssa-lower: post-incr index on non-array {other:?}"),
    };
    // An `any[]` element is a NaN-box: the raw add below would add to
    // its bits, and the unchecked slot store skips everything an
    // element write can mean (an index past the end, an accessor, an
    // arguments object's own index in its props bag). The keyed lane
    // does the §13.4 update through the element kernels.
    if elem_ty == Type::Any {
        let boxed = ctx.box_to_any(arr_val);
        return crate::ssa_lower_post_incr_keyed::lower_index_any_keyed(
            ctx, eid, index, boxed, step,
        );
    }
    let idx_val = ctx.lower_index_operand(index);
    let (offset_base, offset) = ctx.emit_arr_slot_byte_offset(arr_val, idx_val, 3, is_non_deque);
    let cur_block = ctx.cur_block;
    let old = ctx.f.append_inst(
        cur_block,
        InstKind::LoadDyn(elem_ty, offset_base, offset),
        elem_ty,
        None,
    );
    let (one, op) = incr_op_one(elem_ty, step.is_inc);
    let cur_block = ctx.cur_block;
    let new_v = ctx.f.append_inst(
        cur_block,
        InstKind::BinOp(op, Operand::Value(old), one),
        elem_ty,
        None,
    );
    ctx.f.append_void(
        cur_block,
        InstKind::StoreDyn(Operand::Value(new_v), offset_base, offset),
    );
    if step.is_prefix {
        Operand::Value(new_v)
    } else {
        Operand::Value(old)
    }
}
