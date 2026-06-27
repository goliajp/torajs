//! P0.6 / P0.7 / P0.8 — Any-aware BinOp dispatch pulled out of
//! [`crate::ssa_lower::lower_binop_inner`] as chunk-86 of the
//! decomp.
//!
//! `Add` / `Sub` / `Mul` / `Div` / `Mod` and the ordering compares
//! (`Lt` / `Le` / `Gt` / `Ge`) pack each operand as `(tag,
//! value-as-i64)` and call into the matching runtime helper:
//!
//! - **Add** → `__torajs_any_add(lt, lv, rt, rv)` (with
//!   ToPrimitive→ToString fallback per spec §13.15.3).
//! - **Sub/Mul/Div/Mod** → `__torajs_any_arith(opcode, lt, lv, rt, rv)`.
//! - **Lt/Le/Gt/Ge** → `__torajs_any_compare(opcode, lt, lv, rt, rv)`
//!   (Bool result).
//!
//! Returns `Some` when either operand is `Type::Any`; falls through
//! (`None`) so the typed-tier BinOp path handles concrete pairs.

use crate::ast::BinOp as AstBinOp;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    op: AstBinOp,
    a: Operand,
    b: Operand,
) -> Option<Operand> {
    if !matches!(
        op,
        AstBinOp::Add
            | AstBinOp::Sub
            | AstBinOp::Mul
            | AstBinOp::Div
            | AstBinOp::Mod
            | AstBinOp::Lt
            | AstBinOp::Le
            | AstBinOp::Gt
            | AstBinOp::Ge
    ) {
        return None;
    }
    let a_ty = ctx.operand_ty(&a);
    let b_ty = ctx.operand_ty(&b);
    if !matches!(a_ty, Type::Any) && !matches!(b_ty, Type::Any) {
        return None;
    }
    let (lt, lv) = pack_to_tag_value(ctx, a, a_ty);
    let (rt, rv) = pack_to_tag_value(ctx, b, b_ty);
    let r = match op {
        AstBinOp::Add => {
            let cur_block = ctx.cur_block;
            ctx.f.append_inst(
                cur_block,
                InstKind::Call(ctx.intrinsics.any_add, vec![lt, lv, rt, rv]),
                Type::Any,
                None,
            )
        }
        AstBinOp::Sub | AstBinOp::Mul | AstBinOp::Div | AstBinOp::Mod => {
            let op_code: i64 = match op {
                AstBinOp::Sub => 0,
                AstBinOp::Mul => 1,
                AstBinOp::Div => 2,
                AstBinOp::Mod => 3,
                _ => unreachable!(),
            };
            let cur_block = ctx.cur_block;
            ctx.f.append_inst(
                cur_block,
                InstKind::Call(
                    ctx.intrinsics.any_arith,
                    vec![Operand::ConstI64(op_code), lt, lv, rt, rv],
                ),
                Type::Any,
                None,
            )
        }
        AstBinOp::Lt | AstBinOp::Le | AstBinOp::Gt | AstBinOp::Ge => {
            let op_code: i64 = match op {
                AstBinOp::Lt => 0,
                AstBinOp::Le => 1,
                AstBinOp::Gt => 2,
                AstBinOp::Ge => 3,
                _ => unreachable!(),
            };
            let cur_block = ctx.cur_block;
            ctx.f.append_inst(
                cur_block,
                InstKind::Call(
                    ctx.intrinsics.any_compare,
                    vec![Operand::ConstI64(op_code), lt, lv, rt, rv],
                ),
                Type::Bool,
                None,
            )
        }
        _ => unreachable!(),
    };
    Some(Operand::Value(r))
}

fn pack_to_tag_value(ctx: &mut LowerCtx<'_>, op_v: Operand, op_ty: Type) -> (Operand, Operand) {
    if matches!(op_ty, Type::Any) {
        // Any-typed operand: read tag + value via shim (Step 7c
        // layout-decoupling — was inline +8/+16 direct-offset Load).
        let cur_block = ctx.cur_block;
        let tag = ctx.f.append_inst(
            cur_block,
            InstKind::Call(ctx.intrinsics.any_unbox_tag, vec![op_v]),
            Type::I64,
            None,
        );
        let cur_block = ctx.cur_block;
        let value = ctx.f.append_inst(
            cur_block,
            InstKind::Call(ctx.intrinsics.any_unbox_value, vec![op_v]),
            Type::I64,
            None,
        );
        return (Operand::Value(tag), Operand::Value(value));
    }
    // Concrete: same tag/value packing as box_to_any.
    let (tag, value): (i64, Operand) = match op_ty {
        Type::I64 | Type::I32 => (2, op_v),
        Type::F64 => {
            let cur_block = ctx.cur_block;
            let bits =
                ctx.f
                    .append_inst(cur_block, InstKind::BitCastF64ToI64(op_v), Type::I64, None);
            (3, Operand::Value(bits))
        }
        Type::Bool => {
            let cur_block = ctx.cur_block;
            let zext = ctx
                .f
                .append_inst(cur_block, InstKind::ZExtBoolToI64(op_v), Type::I64, None);
            (1, Operand::Value(zext))
        }
        Type::Ptr if matches!(op_v, Operand::ConstPtrNull) => (0, Operand::ConstI64(0)),
        t if t.is_refcounted() => (4, op_v),
        _ => (0, Operand::ConstI64(0)),
    };
    (Operand::ConstI64(tag), value)
}
