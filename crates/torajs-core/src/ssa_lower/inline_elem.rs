//! Arrays that hold a value class in place (`Type::Inline`).
//!
//! Element `i` of such an array is the instance's fields, 8 bytes each
//! in layout order, at `data + i * stride` with `stride = 8 * fields`;
//! there is no header and no word that spells a missing element. An
//! element read answers the element's address, borrowed from the
//! array's data block, and a field read loads through it. Pass 0.5's
//! value-class census admits a class only when no code can tell such
//! an element from a cell, and pass 2.5's value-class check holds the
//! SSA to that; the lowering here asserts the rest of the census's
//! promises it relies on.
//!
//! A read past the end answers the generic undefined cell typed as the
//! element, as a read of a cell array does, so the field read that
//! follows raises the same TypeError, worded the same.

use crate::ast::{ExprId, PropKey};
use crate::ssa::{BinOp, BlockId, Function, InstKind, Operand, StructId, Type, ValueId};
use crate::ssa_lower::LowerCtx;

#[cfg(test)]
mod tests;

/// The bytes one element of `elem` takes in an array's data block.
pub(crate) fn elem_stride(struct_layouts: &[Vec<(PropKey, Type)>], elem: Type) -> u64 {
    match elem {
        Type::Inline(sid) => 8 * struct_layouts[sid.0 as usize].len() as u64,
        _ => 8,
    }
}

/// `idx * stride` appended to `b`: a shift for a power of two, else a
/// multiply.
pub(crate) fn emit_scaled_index(
    f: &mut Function,
    b: BlockId,
    idx: Operand,
    stride: u64,
) -> ValueId {
    let op = if stride.is_power_of_two() {
        let log2 = i64::from(stride.trailing_zeros());
        InstKind::BinOp(BinOp::Shl, idx, Operand::ConstI64(log2))
    } else {
        InstKind::BinOp(BinOp::Mul, idx, Operand::ConstI64(stride as i64))
    };
    f.append_inst(b, op, Type::I64, None)
}

/// The address `data + off` of an element of `sid`, appended to `b`.
pub(crate) fn emit_elem_addr(
    f: &mut Function,
    b: BlockId,
    data: Operand,
    off: Operand,
    sid: StructId,
) -> ValueId {
    let base = f.append_inst(b, InstKind::PtrToInt(data), Type::I64, None);
    let addr = f.append_inst(
        b,
        InstKind::BinOp(BinOp::Add, Operand::Value(base), off),
        Type::I64,
        None,
    );
    f.append_inst(
        b,
        InstKind::IntToPtr(Operand::Value(addr)),
        Type::Inline(sid),
        None,
    )
}

/// Element `idx` of the inline array `arr`, at the current block. The
/// caller has settled the bounds.
pub(crate) fn elem_addr(
    ctx: &mut LowerCtx<'_>,
    arr: Operand,
    idx: Operand,
    sid: StructId,
    is_non_deque: bool,
) -> ValueId {
    let stride = elem_stride(ctx.struct_layouts, Type::Inline(sid));
    let (data, off) = ctx.emit_arr_slot_byte_offset(arr, idx, stride, is_non_deque);
    emit_elem_addr(ctx.f, ctx.cur_block, data, off, sid)
}

/// The byte offset and type of field `name` inside an element of
/// `sid`. The census admits only classes whose fields are all scalar
/// slots read by their declared names.
pub(crate) fn field_slot(
    struct_layouts: &[Vec<(PropKey, Type)>],
    sid: StructId,
    name: &str,
) -> (u64, Type) {
    let layout = &struct_layouts[sid.0 as usize];
    let (idx, ty) = (layout.iter().enumerate())
        .find_map(|(i, (k, t))| (k == name).then_some((i, *t)))
        .unwrap_or_else(|| panic!("ssa-lower: inline element of sid {} has no `{name}`", sid.0));
    assert!(ty.is_copy(), "ssa-lower: inline field `{name}` is {ty:?}");
    (8 * idx as u64, ty)
}

/// `view.name` on an inline element. A view bound from a read past the
/// end holds the undefined cell, which the receiver guard turns into
/// the TypeError the read of a cell array raises.
pub(crate) fn lower_field(
    ctx: &mut LowerCtx<'_>,
    obj: ExprId,
    view: Operand,
    sid: StructId,
    name: &str,
) -> Operand {
    crate::ssa_lower_nullable_guard::emit_undefable_heap_guard(ctx, obj, &view);
    let (off, ty) = field_slot(ctx.struct_layouts, sid, name);
    let cb = ctx.cur_block;
    Operand::Value(
        ctx.f
            .append_inst(cb, InstKind::Load(ty, view, off), ty, None),
    )
}

/// A `const t = xs[i]` view is an address into the array's data block,
/// so it cannot take a stake as a borrowed cell does: nothing between
/// the binding and its last read may grow or release the array, and
/// the binding is never written. The census proves both; this is the
/// lowering's own reading of the same body
/// ([`crate::ssa_lower::alias_hazard`], with an append counted as a
/// move of the buffer).
pub(crate) fn assert_view_binding(ctx: &LowerCtx<'_>, name: &str, init: ExprId, init_ty: Type) {
    if !matches!(init_ty, Type::Inline(_)) {
        return;
    }
    assert!(
        !ctx.proofs.moved_view_inits.contains(&init),
        "internal error: the inline view `{name}` is read after its array may move"
    );
    assert!(
        !ctx.prereserve.body_reassigns(name),
        "internal error: the inline view `{name}` is rebound"
    );
}
