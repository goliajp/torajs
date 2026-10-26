//! What a fn-typed slot takes besides a closure of its own type,
//! split out of `ssa_lower_assign_slot_fit`.

use crate::ast::ExprId;
use crate::ssa::Type;

/// An `any` value stored into a fn-typed binding (`n = raw`): the
/// checked cast takes the closure cell out of the box, as the let
/// initializer's lane does. The checker joins the binding to the
/// boxed-entry census wherever the value's face can differ from the
/// slot's, so its calls unbox per the stored value's own face.
pub(super) fn any_into_closure_slot(slot_ty: Type, v_ty: Type) -> bool {
    matches!(slot_ty, Type::Closure(_)) && v_ty == Type::Any
}

/// A top-level function's bare pointer assigned into a binding that
/// holds closures of the same signature (`let k = mk(); k = f`): the
/// slot takes the function's canonical closure cell, as a closure
/// parameter does. The stores the AST can see arrive wrapped already
/// (`ast::fn_assign_rhs_wrap`); a signature that differs (number
/// widths settled apart) has no cell to hand over.
pub(super) fn fn_pointer_into_closure_slot(slot_ty: Type, v_ty: Type) -> bool {
    matches!((slot_ty, v_ty), (Type::Closure(a), Type::FnSig(b)) if a == b)
}

/// Does the slot written `value` admit `undefined`? The checker admitted
/// the write, so a value it typed as admitting `undefined` (an element
/// of `(F | undefined)[]`) says the slot does too.
pub(crate) fn undefined_admitted(ctx: &crate::ssa_lower::LowerCtx<'_>, value: ExprId) -> bool {
    matches!(
        ctx.expr_types.get(&value),
        Some(crate::check::Type::Nullable(_) | crate::check::Type::Undefined)
    )
}

/// An `any` value written into a function slot that admits `undefined`
/// (`F | undefined`; an element of `(F | undefined)[]` is held boxed):
/// a function's box is its cell already, the `undefined` box becomes
/// the slot's undefined cell, which `=== undefined`, `typeof` and the
/// call guard read, and the `null` box becomes the slot's null pointer,
/// as a `null` literal stores it. Stake-neutral: that cell is immortal.
pub(crate) fn any_into_undefable_closure(
    ctx: &mut crate::ssa_lower::LowerCtx<'_>,
    slot_ty: Type,
    v: crate::ssa::Operand,
) -> crate::ssa::Operand {
    use crate::ssa::{IPred, InstKind, Operand};
    let cb = ctx.cur_block;
    let bits = ctx
        .f
        .append_inst(cb, InstKind::PtrToInt(v), Type::I64, None);
    let undef = torajs_anyvalue::nanbox::VALUE_UNDEFINED as i64;
    let is_undef = ctx.cmp(IPred::Eq, Operand::Value(bits), Operand::ConstI64(undef));
    let cell = ctx
        .str_undef_sentinel_for(slot_ty)
        .expect("a closure slot spells undefined with the generic cell");
    let cb = ctx.cur_block;
    let word = ctx
        .f
        .append_inst(cb, InstKind::PtrToInt(cell), Type::I64, None);
    let picked = ctx.select_i64(
        is_undef,
        Operand::Value(word),
        Operand::Value(bits),
        "__fn_undef_word",
    );
    let null = torajs_anyvalue::nanbox::VALUE_NULL as i64;
    let is_null = ctx.cmp(IPred::Eq, Operand::Value(bits), Operand::ConstI64(null));
    let picked = ctx.select_i64(is_null, Operand::ConstI64(0), picked, "__fn_null_word");
    let cb = ctx.cur_block;
    Operand::Value(
        ctx.f
            .append_inst(cb, InstKind::IntToPtr(picked), slot_ty, None),
    )
}
