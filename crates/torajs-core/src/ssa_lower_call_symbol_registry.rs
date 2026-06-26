//! `Symbol.for(key)` / `Symbol.keyFor(s)` registry helpers pulled out
//! of [`crate::ssa_lower::lower_expr_inner`] `Expr::Call` dispatch as
//! chunk-23 of the `Expr::Call` god-arm decomp (chunks 1-22 = Arr
//! higher-order + Map dispatch + Set dispatch + Arr.push + Number
//! instance methods + bare-name globals + Str regex methods + Number
//! namespace + Array.from + Arr predicate iter + Arr.flatMap +
//! Object.entries + fn-indirect + Number/String/Boolean coercion +
//! universal methods + closure-local + Object.values + Object.keys +
//! Object.getPrototypeOf + Object.assign + Bun runtime cluster +
//! Reflect.get).
//!
//! T-13.b (v0.4.0) — direct delegation to the runtime registry
//! intrinsics `symbol_for` / `symbol_key_for`. `Symbol.for(key)` →
//! `Type::Symbol`, `Symbol.keyFor(s)` → `Type::Str`.
//!
//! Trailing args past `args[0]` are spec-lowered for side-effect
//! parity per the S272 idiom (check.rs S259 already typecheck-drops
//! them). Mirrors S255/S277/S278/S297 sibling arms.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let (ns_id, m_name) = match ctx.ast.get_expr(callee) {
        Expr::Member { obj, name } => (*obj, name.clone()),
        _ => return None,
    };
    if m_name != "for" && m_name != "keyFor" {
        return None;
    }
    let Expr::Ident(ns) = ctx.ast.get_expr(ns_id) else {
        return None;
    };
    if ns != "Symbol" {
        return None;
    }
    if args.is_empty() {
        return None;
    }
    let arg_op = ctx.lower_expr(args[0]);
    ctx.consume_if_ident(args[0]);
    // S302 — lower-and-drop trailing args[1..] per S272 idiom so
    // step()-style side-effect exprs fire per ES eval-then-discard
    // (check.rs S259 already typecheck-dropped). Mirrors S255/S277/
    // S278/S297 sibling arms.
    for &a in args.iter().skip(1) {
        let _ = ctx.lower_expr(a);
    }
    let (fid, ret_ty) = if m_name == "for" {
        (ctx.intrinsics.symbol_for, Type::Symbol)
    } else {
        (ctx.intrinsics.symbol_key_for, Type::Str)
    };
    let cur_block = ctx.cur_block;
    let v = ctx
        .f
        .append_inst(cur_block, InstKind::Call(fid, vec![arg_op]), ret_ty, None);
    Some(Operand::Value(v))
}
