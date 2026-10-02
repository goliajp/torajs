//! `arr.length++` / `--arr.length` — moved verbatim out of
//! `ssa_lower_post_incr.rs` when that file reached its 500-line limit.

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_post_incr::{Step, incr_op_one};

/// The Arr-length lane of `ssa_lower_post_incr::lower_member` — old len loads inline
/// (read-path twin at `ssa_lower_member_typed_props`), the stepped
/// value stores through the same helper split as
/// `lower_arr_length_assign` (scalar elem kinds take the truncate
/// helper, refcounted kinds the full resize; tag 2 = raw i64 pair,
/// `define_length` precedent). The helper can throw (readonly
/// length / invalid value), hence the throw check.
pub(crate) fn lower_arr_length(
    ctx: &mut LowerCtx<'_>,
    obj: ExprId,
    obj_val: Operand,
    obj_ty: Type,
    step: Step,
) -> Operand {
    crate::ssa_lower_nullable_guard::emit_nullable_arr_guard(ctx, obj, &obj_val);
    let cur_block = ctx.cur_block;
    let old = ctx.f.append_inst(
        cur_block,
        InstKind::Load(Type::I64, obj_val, crate::ssa_lower::ARR_LEN_OFF),
        Type::I64,
        None,
    );
    let (one, op) = incr_op_one(Type::I64, step.is_inc);
    let cur_block = ctx.cur_block;
    let new_v = ctx.f.append_inst(
        cur_block,
        InstKind::BinOp(op, Operand::Value(old), one),
        Type::I64,
        None,
    );
    let elem_ty = match obj_ty {
        Type::Arr(arr_id) => ctx.arr_layouts[arr_id.0 as usize],
        _ => unreachable!("gated on Type::Arr above"),
    };
    let mut argv = vec![obj_val, Operand::ConstI64(2), Operand::Value(new_v)];
    let helper = if matches!(elem_ty, Type::I64 | Type::F64 | Type::Bool) {
        argv.push(crate::ssa_lower_index_undef::typed_hole_raw(elem_ty));
        ctx.intrinsics.arr_set_length_truncate_scalar
    } else {
        ctx.intrinsics.arr_set_length_any
    };
    let cur_block = ctx.cur_block;
    ctx.f.append_void(cur_block, InstKind::Call(helper, argv));
    ctx.emit_throw_check(None);
    if step.is_prefix {
        Operand::Value(new_v)
    } else {
        Operand::Value(old)
    }
}
