//! S205 + S228 `Math.{pow|atan2|imul}` <2-arg / `undefined`-arg fold
//! pulled out of [`crate::ssa_lower::lower_expr_inner`]'s `Expr::Call`
//! god-arm as chunk-49 of the decomp (chunks 1-48 = ... + S203/S227
//! Math unary undef fold).
//!
//! - **S205** — `Math.pow` / `Math.atan2` / `Math.imul` with fewer
//!   than 2 args per ES §21.3.2.{5,19,26}:
//!     - `pow` / `atan2`: `ToNumber(undefined) = NaN` → NaN
//!     - `imul`: `ToUint32(undefined) = 0` → `imul = 0` (as f64)
//!   Skips the helper Call entirely, emits the static result.
//! - **S228** — same fold extended to the 2-arg shape when either
//!   operand is statically `Undefined`. The `check::S228` carve-out
//!   admits the call; folding here avoids lowering the `ConstPtrNull`
//!   undef sentinel into the helper's `f64` / `i32` ABI.
//! - **S274** — eval the non-undef args so side-effect exprs still
//!   fire (silent-drop pre-S274 was a regression). Undef positions
//!   skip `lower_expr` (ConstPtrNull sentinel can't enter the
//!   helper's f64/i32 ABI).
//!
//! Returns `Some(op)` on hit; `None` on miss (callee not the
//! `Math.{pow|atan2|imul}` Member-Ident shape, or 2 non-undef args
//! present — the normal path takes over).

use crate::ast::{Expr, ExprId};
use crate::check::{self as check_mod};
use crate::ssa::Operand;
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let bin_undef = args.len() == 2
        && (matches!(
            ctx.expr_types.get(&args[0]),
            Some(check_mod::Type::Undefined)
        ) || matches!(
            ctx.expr_types.get(&args[1]),
            Some(check_mod::Type::Undefined)
        ));
    if args.len() >= 2 && !bin_undef {
        return None;
    }
    let Expr::Member {
        obj: ns_id,
        name: m_name,
    } = ctx.ast.get_expr(callee)
    else {
        return None;
    };
    if !matches!(m_name.as_str(), "pow" | "atan2" | "imul") {
        return None;
    }
    let m_name = m_name.clone();
    let ns_id = *ns_id;
    let Expr::Ident(ns) = ctx.ast.get_expr(ns_id) else {
        return None;
    };
    if ns != "Math" {
        return None;
    }
    if bin_undef {
        for &a in args {
            if !matches!(ctx.expr_types.get(&a), Some(check_mod::Type::Undefined)) {
                let _ = ctx.lower_expr(a);
            }
        }
    }
    if m_name == "imul" {
        return Some(Operand::ConstF64(0.0));
    }
    Some(Operand::ConstF64(f64::NAN))
}
