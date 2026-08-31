//! `Math.hypot(...args)` variadic — `sqrt(sum of args²)` lower pulled
//! out of [`crate::ssa_lower::lower_expr_inner`]'s `Expr::Call`
//! god-arm as chunk-47 of the decomp (chunks 1-46 = ... + Number(x)
//! /String(x) dead-code removal).
//!
//! Lower as `sqrt(sum of args²)` via `BinOp::FMul` per arg + folded
//! `BinOp::FAdd` accumulator + `__torajs_math_sqrt` call. Three
//! short-circuits sit in front of the accumulator:
//!
//! - **V3-18 m1.h.56** — 0-arg returns `+0` per ES §21.3.2.18
//!   (identity element of the reduction).
//! - **S271** — any statically-`Undefined` arg propagates NaN per
//!   ES §21.3.2.18 (ToNumber on each arg, undef → NaN, sum²+sqrt
//!   containing NaN → NaN). We eval-and-drop the non-undef args so
//!   trailing side-effect exprs still fire, then return
//!   `ConstF64(NaN)` so the `ConstPtrNull` undef sentinel never
//!   reaches `coerce_to_f64`.
//! - normal path — `coerce_to_f64` each arg, fold pairwise
//!   `arg² + acc` chain, finalise with `math_sqrt(sum)`.
//!
//! Returns `Some(op)` on hit; `None` on miss (callee not the
//! `Math.hypot` Member-Ident shape).

use crate::ast::{Expr, ExprId};
use crate::check::{self as check_mod};
use crate::ssa::{BinOp as SsaBinOp, InstKind, Operand, Type};
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
    if m_name != "hypot" {
        return None;
    }
    let ns_id = *ns_id;
    let Expr::Ident(ns) = ctx.ast.get_expr(ns_id) else {
        return None;
    };
    if ns != "Math" {
        return None;
    }
    if args.is_empty() {
        return Some(Operand::ConstF64(0.0));
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
    let arg_ids: Vec<ExprId> = args.to_vec();
    let mut acc: Option<Operand> = None;
    for aid in arg_ids.iter() {
        // §21.3.2.18 step 2 ToNumber — `lower_to_number_operand`
        // passes an already-numeric operand straight through, so the
        // typed tier keeps its shape and nothing boxes.
        let raw = ctx.lower_to_number_operand(*aid);
        let v = ctx.coerce_to_f64(raw);
        let cur_block = ctx.cur_block;
        let sq = ctx.f.append_inst(
            cur_block,
            InstKind::BinOp(SsaBinOp::FMul, v, v),
            Type::F64,
            None,
        );
        let sq_op = Operand::Value(sq);
        acc = Some(match acc {
            None => sq_op,
            Some(prev) => {
                let cur_block = ctx.cur_block;
                let s = ctx.f.append_inst(
                    cur_block,
                    InstKind::BinOp(SsaBinOp::FAdd, prev, sq_op),
                    Type::F64,
                    None,
                );
                Operand::Value(s)
            }
        });
    }
    let sum = acc.expect("Math.hypot acc must have been set");
    let cur_block = ctx.cur_block;
    let r = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.math_sqrt, vec![sum]),
        Type::F64,
        None,
    );
    Some(Operand::Value(r))
}
