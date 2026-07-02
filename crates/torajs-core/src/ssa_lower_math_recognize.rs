//! Math intrinsic-arity recognizers for `LowerCtx<'a>` extracted from
//! `ssa_lower.rs` chunk 383 — Path A.3-batch4.
//!
//! Two predicate helpers that classify a resolved FuncId against the
//! Math intrinsic table by arity:
//!
//! - `is_math_unary(fid)`  — matches Math.{sqrt / abs / floor / ceil /
//!   log / exp / sign / round / trunc / sin / cos / tan / asin / acos /
//!   atan / log2 / log10 / cbrt / sinh / cosh / tanh / asinh / acosh /
//!   atanh / expm1 / log1p / fround / f16round}.
//! - `is_math_binary(fid)` — matches Math.{pow / min / max / atan2}.
//!
//! Callers use these to route from a generic call site into the
//! specialized Math emit paths that map to LLVM intrinsics directly.
//!
//! Method bodies are byte-for-byte preserved from the source; the
//! sibling reaches LowerCtx fields via `impl<'a> super::LowerCtx<'a>`,
//! so call sites need zero edits.

use crate::ssa::FuncId;
use crate::ssa_lower::LowerCtx;

impl<'a> LowerCtx<'a> {
    pub(crate) fn is_math_unary(&self, fid: FuncId) -> bool {
        fid == self.intrinsics.math_sqrt
            || fid == self.intrinsics.math_abs
            || fid == self.intrinsics.math_floor
            || fid == self.intrinsics.math_ceil
            || fid == self.intrinsics.math_log
            || fid == self.intrinsics.math_exp
            || fid == self.intrinsics.math_sign
            || fid == self.intrinsics.math_round
            || fid == self.intrinsics.math_trunc
            || fid == self.intrinsics.math_sin
            || fid == self.intrinsics.math_cos
            || fid == self.intrinsics.math_tan
            || fid == self.intrinsics.math_asin
            || fid == self.intrinsics.math_acos
            || fid == self.intrinsics.math_atan
            || fid == self.intrinsics.math_log2
            || fid == self.intrinsics.math_log10
            || fid == self.intrinsics.math_cbrt
            || fid == self.intrinsics.math_sinh
            || fid == self.intrinsics.math_cosh
            || fid == self.intrinsics.math_tanh
            || fid == self.intrinsics.math_asinh
            || fid == self.intrinsics.math_acosh
            || fid == self.intrinsics.math_atanh
            || fid == self.intrinsics.math_expm1
            || fid == self.intrinsics.math_log1p
            || fid == self.intrinsics.math_fround
            || fid == self.intrinsics.math_f16round
    }

    pub(crate) fn is_math_binary(&self, fid: FuncId) -> bool {
        fid == self.intrinsics.math_pow
            || fid == self.intrinsics.math_min
            || fid == self.intrinsics.math_max
            || fid == self.intrinsics.math_atan2
    }
}
