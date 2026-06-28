//! BigInt × BigInt BinOp dispatch extracted from
//! [`crate::ssa_lower_binop_inner::lower`] (chunk 187 —
//! continuation of file-size debt cleanup, mirrors chunks
//! 185/186 shape).
//!
//! T-25 — Routes pure (`BigInt op BigInt`) pairs to the
//! `__torajs_bigint_*` runtime helpers. Add/Sub/Mul return a
//! fresh BigInt; the throwing ops (Div/Mod/Pow/Shl/Shr) flag
//! `ctx.bigint_op_may_throw` so the enclosing Expr::BinOp arm
//! emits a throw-check AFTER dropping refcounted operands.
//! Comparisons go through `bigint_cmp` + ICmp on the int
//! ordering result; six predicates supported (Lt/Gt/Le/Ge/Eq/
//! Neq).
//!
//! Returns `Some(Operand)` when handled, `None` when this
//! path doesn't apply (operand types not both BigInt, OR op
//! outside the arith/cmp set). Caller falls through unchanged
//! on `None`.

use crate::ast::BinOp as AstBinOp;
use crate::ssa::{IPred, InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_lower(
    ctx: &mut LowerCtx,
    op: AstBinOp,
    a: Operand,
    b: Operand,
) -> Option<Operand> {
    let a_ty = ctx.operand_ty(&a);
    let b_ty = ctx.operand_ty(&b);
    if a_ty != Type::BigInt || b_ty != Type::BigInt {
        return None;
    }
    let arith = match op {
        AstBinOp::Add => Some(ctx.intrinsics.bigint_add),
        AstBinOp::Sub => Some(ctx.intrinsics.bigint_sub),
        AstBinOp::Mul => Some(ctx.intrinsics.bigint_mul),
        AstBinOp::Div => Some(ctx.intrinsics.bigint_div),
        AstBinOp::Mod => Some(ctx.intrinsics.bigint_mod),
        AstBinOp::Pow => Some(ctx.intrinsics.bigint_pow),
        AstBinOp::BitAnd => Some(ctx.intrinsics.bigint_and),
        AstBinOp::BitOr => Some(ctx.intrinsics.bigint_or),
        AstBinOp::BitXor => Some(ctx.intrinsics.bigint_xor),
        AstBinOp::Shl => Some(ctx.intrinsics.bigint_shl),
        AstBinOp::Shr => Some(ctx.intrinsics.bigint_shr),
        _ => None,
    };
    if let Some(fid) = arith {
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(fid, vec![a, b]),
            Type::BigInt,
            None,
        );
        // P7.4-a-b — these bigint helpers can call
        // __torajs_throw_range_error (divide-by-zero /
        // negative exponent / shift too large). Flag it so
        // the enclosing Expr::BinOp arm emits the throw-
        // check AFTER dropping the refcounted operands;
        // emitting it here would split the block before the
        // a/b drops and strand them.
        if matches!(
            op,
            AstBinOp::Div | AstBinOp::Mod | AstBinOp::Pow | AstBinOp::Shl | AstBinOp::Shr
        ) {
            ctx.bigint_op_may_throw = true;
        }
        return Some(Operand::Value(v));
    }
    if matches!(
        op,
        AstBinOp::Lt | AstBinOp::Gt | AstBinOp::Le | AstBinOp::Ge | AstBinOp::Eq | AstBinOp::Neq
    ) {
        let c = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.bigint_cmp, vec![a, b]),
            Type::I64,
            None,
        );
        let pred = match op {
            AstBinOp::Lt => IPred::Slt,
            AstBinOp::Gt => IPred::Sgt,
            AstBinOp::Le => IPred::Sle,
            AstBinOp::Ge => IPred::Sge,
            AstBinOp::Eq => IPred::Eq,
            AstBinOp::Neq => IPred::Ne,
            _ => unreachable!(),
        };
        let r = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::ICmp(pred, Operand::Value(c), Operand::ConstI64(0)),
            Type::Bool,
            None,
        );
        return Some(Operand::Value(r));
    }
    None
}
