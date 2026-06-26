//! S203 + S227 `Math.<unary>` 0-arg / single-`undefined`-arg fold
//! pulled out of [`crate::ssa_lower::lower_expr_inner`]'s
//! `Expr::Call` god-arm as chunk-48 of the decomp (chunks 1-47 = ...
//! + Math.hypot).
//!
//! - **S203** — `Math.<unary>()` 0-arg per ES §21.3.2.* step 1:
//!   `ToNumber(undefined) = NaN`, then each method returns NaN
//!   unchanged. Short-circuit so no helper Call is emitted (the C
//!   intrinsic signature requires the f64 operand).
//! - **S227** — extend the same fold to an explicit `undefined`
//!   1-arg shape (`Math.floor(undefined)`). The `check::S227`
//!   carve-out lets the call through the type gate; we skip
//!   `lower_expr` so the `ConstPtrNull` undef sentinel never
//!   reaches the helper's f64 ABI.
//!
//! `clz32` is the lone exception — it applies `ToUint32` (not
//! `ToNumber`), and `ToUint32(undefined) = +0`, so `Math.clz32()`
//! returns `32` (count of leading zero bits in the 32-bit `0`).
//!
//! Returns `Some(op)` on hit; `None` on miss (callee not the
//! `Math.<unary>` Member-Ident shape, or non-undef args present, or
//! method outside the allowlist).

use crate::ast::{Expr, ExprId};
use crate::check::{self as check_mod};
use crate::ssa::Operand;
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let arg0_is_undef = args.len() == 1
        && matches!(
            ctx.expr_types.get(&args[0]),
            Some(check_mod::Type::Undefined)
        );
    if !args.is_empty() && !arg0_is_undef {
        return None;
    }
    let Expr::Member {
        obj: ns_id,
        name: m_name,
    } = ctx.ast.get_expr(callee)
    else {
        return None;
    };
    let ns_id = *ns_id;
    let Expr::Ident(ns) = ctx.ast.get_expr(ns_id) else {
        return None;
    };
    if ns != "Math" {
        return None;
    }
    let nan_unary = matches!(
        m_name.as_str(),
        "sqrt"
            | "abs"
            | "floor"
            | "ceil"
            | "log"
            | "exp"
            | "sign"
            | "round"
            | "trunc"
            | "sin"
            | "cos"
            | "tan"
            | "asin"
            | "acos"
            | "atan"
            | "log2"
            | "log10"
            | "cbrt"
            | "sinh"
            | "cosh"
            | "tanh"
            | "asinh"
            | "acosh"
            | "atanh"
            | "expm1"
            | "log1p"
            | "fround"
            | "f16round"
    );
    if nan_unary {
        return Some(Operand::ConstF64(f64::NAN));
    }
    if m_name == "clz32" {
        return Some(Operand::ConstF64(32.0));
    }
    None
}
