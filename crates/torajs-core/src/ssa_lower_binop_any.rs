//! Any-tagged BinOp dispatch (Add/Sub/Mul/Div/Mod/Lt/Le/Gt/Ge with
//! at least one Any-typed operand) extracted from
//! [`crate::ssa_lower_binop_inner`] (chunk 171 — file-size debt
//! cleanup for chunk 160's sibling).
//!
//! Pre-extract this Any-tagged block was 112 LOC inside
//! `ssa_lower_binop_inner::lower`. Body verbatim moves here as
//! `try_lower(ctx, op, a, b) -> Option<Operand>` — returns
//! `Some(r)` when both operands route through the Any-tagged
//! dispatch (Add → any_add with ToPrimitive→ToString fallback;
//! arith → any_arith with op code; ordering → any_compare with op
//! code).

use crate::ast::BinOp as AstBinOp;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_lower(
    ctx: &mut LowerCtx,
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
    let pack = |this: &mut LowerCtx, op_v: Operand, op_ty: Type| -> (Operand, Operand) {
        if matches!(op_ty, Type::Any) {
            let tag = this.f.append_inst(
                this.cur_block,
                InstKind::Call(this.intrinsics.any_unbox_tag, vec![op_v.clone()]),
                Type::I64,
                None,
            );
            let value = this.f.append_inst(
                this.cur_block,
                InstKind::Call(this.intrinsics.any_unbox_value, vec![op_v]),
                Type::I64,
                None,
            );
            return (Operand::Value(tag), Operand::Value(value));
        }
        let (tag, value): (i64, Operand) = match op_ty {
            Type::I64 | Type::I32 => (2, op_v),
            Type::F64 => {
                let bits = this.f.append_inst(
                    this.cur_block,
                    InstKind::BitCastF64ToI64(op_v),
                    Type::I64,
                    None,
                );
                (3, Operand::Value(bits))
            }
            Type::Bool => {
                let zext = this.f.append_inst(
                    this.cur_block,
                    InstKind::ZExtBoolToI64(op_v),
                    Type::I64,
                    None,
                );
                (1, Operand::Value(zext))
            }
            Type::Ptr if matches!(op_v, Operand::ConstPtrNull) => (0, Operand::ConstI64(0)),
            t if t.is_refcounted() => (4, op_v),
            _ => (0, Operand::ConstI64(0)),
        };
        (Operand::ConstI64(tag), value)
    };
    let (lt, lv) = pack(ctx, a, a_ty);
    let (rt, rv) = pack(ctx, b, b_ty);
    let r = match op {
        AstBinOp::Add => ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.any_add, vec![lt, lv, rt, rv]),
            Type::Any,
            None,
        ),
        AstBinOp::Sub | AstBinOp::Mul | AstBinOp::Div | AstBinOp::Mod => {
            let op_code: i64 = match op {
                AstBinOp::Sub => 0,
                AstBinOp::Mul => 1,
                AstBinOp::Div => 2,
                AstBinOp::Mod => 3,
                _ => unreachable!(),
            };
            ctx.f.append_inst(
                ctx.cur_block,
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
            ctx.f.append_inst(
                ctx.cur_block,
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
