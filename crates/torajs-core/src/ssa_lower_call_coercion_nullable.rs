//! ToString for a pointer-shaped `T | null` — the one question
//! [`crate::ssa_lower_call_coercion::emit_to_string_inner`] has no arm
//! for.
//!
//! A pointer-shaped nullable is spelled as bare T all the way down:
//! the slot is the same slot, and its null is the in-band 0 the
//! pointer always had spare. So every arm of the ordinary coercion
//! reads that 0 as a live value of its own type — the Str arm handed
//! it straight back (a null POINTER that printed as `null` and had no
//! `.length`), the join kernel read it as an array cell and produced
//! nothing at all. The two scalars never arrive here; they ride `any`
//! and their arm has always answered "null".
//!
//! §7.1.17 asks null first and ToString second, and that is the whole
//! content of this module.

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// Does this argument hold a pointer-shaped `T | null`, whose null is
/// the in-band 0 no arm of [`emit_to_string_inner`] tests for?
pub(crate) fn is_nullable_ptr(ctx: &LowerCtx<'_>, arg_eid: ExprId, arg_ty: &Type) -> bool {
    matches!(
        ctx.expr_types.get(&arg_eid),
        Some(crate::check::Type::Nullable(_))
    ) && matches!(
        arg_ty,
        Type::Str | Type::Substr | Type::Arr(_) | Type::Obj(_) | Type::FnSig(_) | Type::Closure(_)
    )
}

/// ToString for a pointer-shaped nullable — the shape
/// `coerce_nullable_ptr_to_str` already uses on the `+` path: branch
/// on the in-band 0, answer `null_to_str` on one side and the
/// ordinary coercion on the other, merge through an alloca'd Str slot
/// (this IR has no phi). Both arms answer an owned Str.
pub(crate) fn emit_nullable_to_string(
    ctx: &mut LowerCtx<'_>,
    arg_eid: ExprId,
    arg_op: Operand,
    arg_ty: Type,
    implicit_tostring: bool,
) -> Operand {
    use crate::ssa::{IPred, Terminator};
    let slot = ctx.alloca(Type::Str, None);
    let null_blk = ctx.f.add_block();
    let live_blk = ctx.f.add_block();
    let merge = ctx.f.add_block();
    let is_null = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Eq, arg_op.clone(), Operand::ConstPtrNull),
        Type::Bool,
        None,
    );
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(is_null),
            then_blk: null_blk,
            else_blk: live_blk,
        },
    );
    ctx.cur_block = null_blk;
    let n = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.null_to_str, vec![]),
        Type::Str,
        None,
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(n), Operand::Value(slot), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(merge));
    ctx.cur_block = live_blk;
    let s = crate::ssa_lower_call_coercion::emit_to_string_inner(
        ctx,
        arg_eid,
        arg_op,
        arg_ty,
        implicit_tostring,
    );
    ctx.f
        .append_void(ctx.cur_block, InstKind::Store(s, Operand::Value(slot), 0));
    ctx.f.set_term(ctx.cur_block, Terminator::Br(merge));
    ctx.cur_block = merge;
    Operand::Value(ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::Str, Operand::Value(slot), 0),
        Type::Str,
        None,
    ))
}
