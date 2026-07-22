//! §7.2.13 steps 6-9 numeric-coercion loose-eq folds split out of
//! [`crate::ssa_lower_binop_loose_eq`] (file-size limit):
//! `String == Number` coerces the string side via
//! `__torajs_str_to_number`; `Boolean == String` coerces the Boolean
//! side to Number first. Both compare numerically as f64.

use crate::ast::BinOp as AstBinOp;
use crate::ssa::{BinOp as SsaBinOp, FPred, InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(super) fn try_lower_str_num(
    ctx: &mut LowerCtx<'_>,
    op: AstBinOp,
    a: Operand,
    b: Operand,
    a_ty: Type,
    b_ty: Type,
) -> Option<Operand> {
    let (str_op, num_op, num_ty) = if a_ty == Type::Str && matches!(b_ty, Type::I64 | Type::F64) {
        (a, b, b_ty)
    } else if b_ty == Type::Str && matches!(a_ty, Type::I64 | Type::F64) {
        (b, a, a_ty)
    } else {
        return None;
    };
    let cur_block = ctx.cur_block;
    let str_num = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.str_to_number, vec![str_op]),
        Type::F64,
        None,
    );
    let num_f64 = if matches!(num_ty, Type::I64) {
        ctx.coerce_to_f64(num_op)
    } else {
        num_op
    };
    let eq = ctx.fcmp(FPred::Oeq, Operand::Value(str_num), num_f64);
    if matches!(op, AstBinOp::LooseNeq) {
        let cur_block = ctx.cur_block;
        let neg = ctx.f.append_inst(
            cur_block,
            InstKind::BinOp(SsaBinOp::Xor, eq, Operand::ConstBool(true)),
            Type::Bool,
            None,
        );
        return Some(Operand::Value(neg));
    }
    Some(eq)
}

pub(super) fn try_lower_bool_str(
    ctx: &mut LowerCtx<'_>,
    op: AstBinOp,
    a: Operand,
    b: Operand,
    a_ty: Type,
    b_ty: Type,
) -> Option<Operand> {
    let (bool_op, str_op) = if a_ty == Type::Bool && b_ty == Type::Str {
        (a, b)
    } else if a_ty == Type::Str && b_ty == Type::Bool {
        (b, a)
    } else {
        return None;
    };
    let cur_block = ctx.cur_block;
    let bool_i64 = ctx
        .f
        .append_inst(cur_block, InstKind::ZExtBoolToI64(bool_op), Type::I64, None);
    let bool_f64 = ctx.coerce_to_f64(Operand::Value(bool_i64));
    let cur_block = ctx.cur_block;
    let str_num = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.str_to_number, vec![str_op]),
        Type::F64,
        None,
    );
    let eq = ctx.fcmp(FPred::Oeq, bool_f64, Operand::Value(str_num));
    if matches!(op, AstBinOp::LooseNeq) {
        let cur_block = ctx.cur_block;
        let neg = ctx.f.append_inst(
            cur_block,
            InstKind::BinOp(SsaBinOp::Xor, eq, Operand::ConstBool(true)),
            Type::Bool,
            None,
        );
        return Some(Operand::Value(neg));
    }
    Some(eq)
}
