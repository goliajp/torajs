//! f64 BinOp dispatch extracted from
//! [`crate::ssa_lower_binop_inner::lower`] (chunk 186 —
//! continuation of file-size debt cleanup, mirrors chunk 185
//! shape).
//!
//! Reached when the parent body determines `is_float` is true
//! (Div / Pow / Mod-not-int-safe / Mul with -0 risk / either
//! operand already `Type::F64`). Bitwise / shift ops short-
//! circuit to `lower_bitwise_int32` per JS spec §7.1.6 ToInt32 +
//! §13.12 (`1 << 31` wraps, `4294967296 | 0` is 0) before the
//! f64 coerce. Remainder is `FRem` (IEEE fmod-shaped) per JS
//! spec §13.10; `!=` / `!==` use `Une` (unordered-or-not-equal)
//! so `NaN !== NaN` is true per spec §7.2.16.
//!
//! Body verbatim — same dispatch tree, same `&mut LowerCtx`
//! method calls (`bin` / `fcmp` / `coerce_to_f64` /
//! `lower_bitwise_int32`) preserved.

use crate::ast::BinOp as AstBinOp;
use crate::ssa::{BinOp as SsaBinOp, FPred, InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn lower_float_path(
    ctx: &mut LowerCtx,
    op: AstBinOp,
    a: Operand,
    b: Operand,
) -> Operand {
    // V3-18 m1.h.40 / L3a-8 — JS spec §7.1.6 ToInt32 / §13.12.x:
    // bitwise ops on Number first ToInt32 each operand. The
    // shared int32-semantics helper truncates f64 via FpToSi
    // then sign-extends the low 32 bits (NaN / out-of-i64-range
    // land as poison — same as the integer bitwise idioms v8 /
    // jsc compile to).
    //
    // V3-18 m1.h.41 — Mod with f64 operands maps to
    // LLVM frem (IEEE fmod-shaped), matching JS spec
    // §13.10 numeric remainder for non-integer Number.
    if matches!(
        op,
        AstBinOp::BitAnd
            | AstBinOp::BitOr
            | AstBinOp::BitXor
            | AstBinOp::Shl
            | AstBinOp::Shr
            | AstBinOp::UShr
    ) {
        return ctx.lower_bitwise_int32(op, a, b);
    }
    let af = ctx.coerce_to_f64(a);
    let bf = ctx.coerce_to_f64(b);
    // ToNumber(undefined) already happened where it is free: the
    // index read's own out-of-range exit answered a plain NaN
    // (`f64_oob_plain_for`, set in [`crate::ssa_lower_binop`]).
    // Undoing the payload here instead — after the value exists —
    // means a branch in the middle of a chain `float_demote` was
    // demoting, and that measured 54% on `array-sum-1m`. What the
    // free path cannot reach is recorded in
    // [`crate::ssa_lower_f64_sentinel_canon`].
    match op {
        AstBinOp::Add => ctx.bin(SsaBinOp::FAdd, af, bf, Type::F64),
        AstBinOp::Sub => ctx.bin(SsaBinOp::FSub, af, bf, Type::F64),
        AstBinOp::Mul => ctx.bin(SsaBinOp::FMul, af, bf, Type::F64),
        AstBinOp::Mod => ctx.bin(SsaBinOp::FRem, af, bf, Type::F64),
        AstBinOp::Div => ctx.bin(SsaBinOp::FDiv, af, bf, Type::F64),
        AstBinOp::Pow => {
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.math_pow, vec![af, bf]),
                Type::F64,
                None,
            );
            Operand::Value(v)
        }
        AstBinOp::Lt => ctx.fcmp(FPred::Olt, af, bf),
        AstBinOp::Gt => ctx.fcmp(FPred::Ogt, af, bf),
        AstBinOp::Le => ctx.fcmp(FPred::Ole, af, bf),
        AstBinOp::Ge => ctx.fcmp(FPred::Oge, af, bf),
        AstBinOp::Eq | AstBinOp::LooseEq => ctx.fcmp(FPred::Oeq, af, bf),
        // V3-18 m1.h.32 — NaN !== NaN must be true per JS
        // spec §7.2.16. FCmp::One (ordered-not-equal)
        // returns false when either operand is NaN; the
        // correct shape is Une (unordered-or-not-equal),
        // which is true if either side is NaN OR the values
        // differ — matches the spec for both NaN and normal
        // numbers.
        AstBinOp::Neq | AstBinOp::LooseNeq => ctx.fcmp(FPred::Une, af, bf),
        // (`AstBinOp::Mod` in the f64 path is handled above
        // via FRem — not repeated here; zero-warn rule.)
        AstBinOp::BitAnd
        | AstBinOp::BitOr
        | AstBinOp::BitXor
        | AstBinOp::Shl
        | AstBinOp::Shr
        | AstBinOp::UShr
        | AstBinOp::LAnd
        | AstBinOp::LOr => unreachable!(),
    }
}
