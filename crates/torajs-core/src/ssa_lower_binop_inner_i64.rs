//! Default i64 numeric / cmp / bitwise BinOp dispatch extracted
//! from [`crate::ssa_lower_binop_inner::lower`] (chunk 185 —
//! file-size debt cleanup, mirrors chunks 179-184 sub-stage
//! extraction shape).
//!
//! This is the fall-through path that runs after every
//! Any-tagged / BigInt / String / f64 / strict-eq / loose-eq /
//! bitwise-on-f64 short-circuit above has been exhausted. Body
//! is verbatim — same dispatch tree, same Mod=0 NaN guard, same
//! `&mut LowerCtx` method calls (`bin` / `cmp` /
//! `lower_bitwise_int32`).
//!
//! Sub-stage extraction matches the 179-184 pattern: zero
//! captured state, pure-fn shape, `(ctx, op, a, b)` signature
//! identical to the parent. Caller becomes a single trailing
//! expression.

use crate::ast::BinOp as AstBinOp;
use crate::ssa::{BinOp as SsaBinOp, IPred, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn lower_i64_default(
    ctx: &mut LowerCtx,
    op: AstBinOp,
    a: Operand,
    b: Operand,
) -> Operand {
    match op {
        AstBinOp::Add => ctx.bin(SsaBinOp::Add, a, b, Type::I64),
        AstBinOp::Sub => ctx.bin(SsaBinOp::Sub, a, b, Type::I64),
        AstBinOp::Mul => ctx.bin(SsaBinOp::Mul, a, b, Type::I64),
        AstBinOp::Div => unreachable!("Div forced into float path above"),
        AstBinOp::Pow => unreachable!("Pow forced into float path above"),
        // V3-18 m1.h.39 — JS spec §13.10: `a % 0` on Number is
        // NaN. LLVM's srem with divisor 0 is UB and tora silently
        // returned 0. Detect a compile-time-zero divisor and
        // emit ConstF64(NaN). Runtime-zero divisor (`a % b` with
        // b loaded from a slot) still falls through to srem; a
        // proper guard needs branching IR + f64 result, which
        // changes types and is deferred.
        //
        // W3 — only reachable for a provably non-negative constant
        // dividend (`mod_int_safe`); every other Mod takes the
        // float path above for -0 correctness.
        AstBinOp::Mod => {
            if matches!(b, Operand::ConstI64(0)) {
                return Operand::ConstF64(f64::NAN);
            }
            ctx.bin(SsaBinOp::SRem, a, b, Type::I64)
        }
        // L3a-8 — JS spec §13.9 / §13.12: bitwise/shift on Number is
        // int32-width even when both operands are integral i64
        // (`1 << 31` wraps negative, `4294967296 | 0` is 0). All six
        // operators share the ToInt32-normalize helper.
        AstBinOp::BitAnd
        | AstBinOp::BitOr
        | AstBinOp::BitXor
        | AstBinOp::Shl
        | AstBinOp::Shr
        | AstBinOp::UShr => ctx.lower_bitwise_int32(op, a, b),
        AstBinOp::Lt => ctx.cmp(IPred::Slt, a, b),
        AstBinOp::Gt => ctx.cmp(IPred::Sgt, a, b),
        AstBinOp::Le => ctx.cmp(IPred::Sle, a, b),
        AstBinOp::Ge => ctx.cmp(IPred::Sge, a, b),
        AstBinOp::Eq | AstBinOp::LooseEq => ctx.cmp(IPred::Eq, a, b),
        AstBinOp::Neq | AstBinOp::LooseNeq => ctx.cmp(IPred::Ne, a, b),
        AstBinOp::LAnd | AstBinOp::LOr => {
            unreachable!("logical && / || handled before lower_binop")
        }
    }
}
