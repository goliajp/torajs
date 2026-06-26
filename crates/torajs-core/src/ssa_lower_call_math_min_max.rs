//! `Math.min(...args)` / `Math.max(...args)` variadic pairwise
//! reduction pulled out of [`crate::ssa_lower::lower_expr_inner`]'s
//! `Expr::Call` god-arm as chunk-50 of the decomp (chunks 1-49 = ...
//! + S205/S228 Math binary undef fold).
//!
//! V3-18 m1.h.24 — handles the full variadic shape:
//!
//! - **0 args** — spec identity per ES §21.3.2.{24,25}: `min → +Inf`,
//!   `max → -Inf`.
//! - **1 arg** — coerce the single operand to F64 (no helper call).
//! - **≥2 args** — pairwise left-to-right reduction
//!   `r = min(a,b); r = min(r,c); ...` via `__torajs_math_min` /
//!   `__torajs_math_max`.
//!
//! Folds:
//!
//! - **S228** — any statically-`Undefined` arg propagates NaN through
//!   min/max per ES §21.3.2.{24,25} (each step `ToNumber`,
//!   `ToNumber(undefined) = NaN`, min/max with NaN → NaN). Returns
//!   `ConstF64(NaN)` without lowering the undef args (the
//!   `ConstPtrNull` undef sentinel can't enter `coerce_to_f64`).
//! - **S274** — eval the non-undef args before short-circuit so
//!   `step()`-style side-effect exprs fire.
//! - **S342** — `Any`-typed arg per ES §21.3.2.{24,25} `ToNumber`:
//!   arbitrary-typed input is accepted. Route `Any` through
//!   `__torajs_anyv_to_number` → F64 instead of `coerce_to_f64`
//!   (which can't handle Any). Applies to the 1-arg fast path AND
//!   the variadic reduction.
//!
//! Returns `Some(op)` on hit; `None` on miss (callee not the
//! `Math.{min|max}` Member-Ident shape).

use crate::ast::{Expr, ExprId};
use crate::check::{self as check_mod};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let Expr::Member {
        obj: ns_id,
        name: m_name,
    } = ctx.ast.get_expr(callee)
    else {
        return None;
    };
    let is_min = m_name == "min";
    if !is_min && m_name != "max" {
        return None;
    }
    let ns_id = *ns_id;
    let Expr::Ident(ns) = ctx.ast.get_expr(ns_id) else {
        return None;
    };
    if ns != "Math" {
        return None;
    }
    if args
        .iter()
        .any(|a| matches!(ctx.expr_types.get(a), Some(check_mod::Type::Undefined)))
    {
        for &a in args {
            if !matches!(ctx.expr_types.get(&a), Some(check_mod::Type::Undefined)) {
                let _ = ctx.lower_expr(a);
            }
        }
        return Some(Operand::ConstF64(f64::NAN));
    }
    if args.is_empty() {
        let identity = if is_min {
            f64::INFINITY
        } else {
            f64::NEG_INFINITY
        };
        return Some(Operand::ConstF64(identity));
    }
    let target = if is_min {
        ctx.intrinsics.math_min
    } else {
        ctx.intrinsics.math_max
    };
    if args.len() == 1 {
        let op = ctx.lower_expr(args[0]);
        return Some(coerce_arg(ctx, op));
    }
    let arg_ids: Vec<ExprId> = args.to_vec();
    let head = ctx.lower_expr(arg_ids[0]);
    let mut acc = coerce_arg(ctx, head);
    for aid in arg_ids.iter().skip(1) {
        let next_op = ctx.lower_expr(*aid);
        let next_v = coerce_arg(ctx, next_op);
        let cur_block = ctx.cur_block;
        let v = ctx.f.append_inst(
            cur_block,
            InstKind::Call(target, vec![acc, next_v]),
            Type::F64,
            None,
        );
        acc = Operand::Value(v);
    }
    Some(acc)
}

fn coerce_arg(ctx: &mut LowerCtx<'_>, op: Operand) -> Operand {
    if ctx.operand_ty(&op) == Type::Any {
        let cur_block = ctx.cur_block;
        let f = ctx.f.append_inst(
            cur_block,
            InstKind::Call(ctx.intrinsics.any_to_number, vec![op]),
            Type::F64,
            None,
        );
        Operand::Value(f)
    } else {
        ctx.coerce_to_f64(op)
    }
}
