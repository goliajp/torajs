//! The `re.lastIndex = v` store, split out of `ssa_lower_assign_member`.

use super::*;

pub(super) fn lower_regex_last_index_assign(
    ctx: &mut LowerCtx<'_>,
    obj_val: Operand,
    value: ExprId,
) -> Operand {
    let v_raw = ctx.lower_expr(value);
    // Chunk 566 — no consume: any_to_number only READS an Any rhs,
    // so a borrow-shape source keeps its stake; an owned Any temp's
    // box releases after the store below.
    let transfers = ctx.expr_transfers_ownership(value);
    // lastIndex is an ordinary data property — the store is
    // uncoerced (`r.lastIndex = 2.9` reads back 2.9); ToLength
    // happens at the regex kernels' consumption sites. An Any RHS
    // goes through ToNumber for the f64 slot.
    let v_ty = ctx.operand_ty(&v_raw);
    let v_keep = v_raw;
    let v_f64 = if v_ty == Type::Any {
        let cur_block = ctx.cur_block;
        let f = ctx.f.append_inst(
            cur_block,
            InstKind::Call(ctx.intrinsics.any_to_number, vec![v_raw]),
            Type::F64,
            None,
        );
        Operand::Value(f)
    } else {
        ctx.coerce_to_f64(v_raw)
    };
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Call(ctx.intrinsics.regex_set_last_index, vec![obj_val, v_f64]),
    );
    if transfers && v_ty.is_refcounted() {
        ctx.emit_drop_value(v_keep, v_ty);
    }
    v_f64
}
