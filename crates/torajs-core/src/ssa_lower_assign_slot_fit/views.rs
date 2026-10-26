//! A split product's `Arr<Substr>` views leaving for an owned
//! `Arr<Str>` slot: materialized in place or copied out.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx};

/// A fresh split product (the expression transfers ownership — the
/// site holds the only reference) is materialized in place: every
/// view becomes an owned string and the same cell is stored. Answers
/// that cell under the slot's `Arr<Str>` type.
pub(crate) fn materialize_fresh_views(
    ctx: &mut LowerCtx<'_>,
    slot_ty: Type,
    v: Operand,
) -> Operand {
    let cur_block = ctx.cur_block;
    let owned = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.arr_substr_materialize_owned, vec![v]),
        slot_ty,
        None,
    );
    Operand::Value(owned)
}

/// A call's fresh `Arr<Substr>` product outside a view home
/// (`ast::let_owned_elems`, "Temporaries") is on its way to a receiver
/// typed by its own annotation — an argument, a literal element, a
/// returned or assigned value, a conditional's branch. It leaves the
/// producing expression as owned strings, under `Arr<Str>`. Only a
/// `split` call's own product is converted in place: any other call
/// answering views (`parts.reverse()`) may be handing back an array a
/// view-typed binding still reads, so it is copied out and the call's
/// stake released.
pub(crate) fn settle_escaping_views(ctx: &mut LowerCtx<'_>, eid: ExprId, v: Operand) -> Operand {
    let v_ty = ctx.operand_ty(&v);
    let Type::Arr(id) = v_ty else {
        return v;
    };
    if ctx.arr_layouts[id.0 as usize] != Type::Substr || ctx.ast.view_home_exprs.contains(&eid) {
        return v;
    }
    let owned_ty = Type::Arr(crate::ssa_lower::intern_arr_layout(
        ctx.arr_layouts,
        Type::Str,
    ));
    let is_split = matches!(ctx.ast.get_expr(eid), Expr::Call { callee, .. }
        if matches!(ctx.ast.get_expr(*callee), Expr::Member { name, .. } if name == "split"));
    if is_split {
        return materialize_fresh_views(ctx, owned_ty, v);
    }
    let copy = copy_views_out(ctx, owned_ty, v);
    ctx.emit_drop_value(v, v_ty);
    copy
}

/// A borrowed view array is copied out through the slice kernel and
/// the copy adopted — views never leave their split block — and the
/// fresh `Arr<Str>` copy is answered. The caller decides what happens
/// to the source's stake (an assignment hands its borrow inc back; a
/// call releases nothing it did not take) and who releases the copy.
pub(crate) fn copy_views_out(ctx: &mut LowerCtx<'_>, slot_ty: Type, v: Operand) -> Operand {
    let cur_block = ctx.cur_block;
    let len = ctx.f.append_inst(
        cur_block,
        InstKind::Load(Type::I64, v, ARR_LEN_OFF),
        Type::I64,
        None,
    );
    let copy = ctx.f.append_inst(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.arr_slice,
            vec![v, Operand::ConstI64(0), Operand::Value(len)],
        ),
        slot_ty,
        None,
    );
    ctx.emit_adopt_copied_range(
        Operand::Value(copy),
        Type::Substr,
        Operand::ConstI64(0),
        Operand::Value(len),
    );
    Operand::Value(copy)
}

/// The assignment face: a fresh rhs is materialized in place; a
/// borrowed one (an alias the borrow inc above staked) is copied out,
/// then the stake on the source is handed back, since the slot holds
/// the copy.
pub(super) fn coerce_arr_views_to_owned(
    ctx: &mut LowerCtx<'_>,
    slot_ty: Type,
    v_ty: Type,
    v: Operand,
    value: ExprId,
) -> Operand {
    if ctx.expr_transfers_ownership(value) {
        return materialize_fresh_views(ctx, slot_ty, v);
    }
    let copy = copy_views_out(ctx, slot_ty, v);
    ctx.emit_drop_value(v, v_ty);
    copy
}
