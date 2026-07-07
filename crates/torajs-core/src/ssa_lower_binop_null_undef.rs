//! ES §7.2.15 nullish strict-equality static folds pulled out of
//! [`crate::ssa_lower::lower_binop_inner`] as chunk-87 of the
//! decomp; rewritten type-based in chunk 612.
//!
//! Both `null` and `undefined` lower to a zero pointer at the SSA
//! layer — a raw ptr-cmp can't tell them apart, and an
//! Undefined/Null-typed BINDING is a Load (not `ConstPtrNull`), so
//! operand-shape checks can't either. Fold on the checker types
//! instead, via the `binop_{left,right}_{undef,null}_id` flags set
//! in `lower_binop_with_ids`:
//!
//! - Undefined vs Null (either order, literal or binding) → `===`
//!   false / `!==` true (spec §6.1.1/§6.1.2 distinct types).
//! - Undefined vs Undefined / Null vs Null → `===` true (both are
//!   single-value types).
//! - One nullish side vs a Str-typed value (RFC 20260707 chunk 2)
//!   → runtime identity cmp against the undefined sentinel cell
//!   (`=== undefined`) or NULL (`=== null`) — a Str slot can hold
//!   either nullish repr.
//! - Anything else → fall through (`None`) to the Any lane /
//!   same-family cmp paths.
//!
//! The pre-612 operand-shape version mistook the `undefined`
//! LITERAL (also ConstPtrNull) for `null`, folding
//! `y === undefined` to false for an Undefined-typed `y`.

use crate::ast::BinOp as AstBinOp;
use crate::ssa::{BinOp as SsaBinOp, IPred, InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    op: AstBinOp,
    a: Operand,
    b: Operand,
) -> Option<Operand> {
    if !matches!(op, AstBinOp::Eq | AstBinOp::Neq) {
        return None;
    }
    let a_undef = ctx.binop_left_undef_id.is_some();
    let b_undef = ctx.binop_right_undef_id.is_some();
    let a_null = ctx.binop_left_null_id.is_some();
    let b_null = ctx.binop_right_null_id.is_some();
    let a_nullish = a_undef || a_null;
    let b_nullish = b_undef || b_null;
    if a_nullish && b_nullish {
        let eq = (a_undef && b_undef) || (a_null && b_null);
        let answer = match op {
            AstBinOp::Eq => eq,
            AstBinOp::Neq => !eq,
            _ => unreachable!(),
        };
        return Some(Operand::ConstBool(answer));
    }
    // RFC 20260707 chunk 2 (+ residual chunk) — one nullish side vs
    // a Str- or Substr-typed value. A Str slot can hold the Str
    // undefined sentinel cell (missed exec/match capture) or NULL
    // (JS null); a Substr slot can hold the Substr-shaped sentinel
    // (string index OOB read): `x === undefined` compares against
    // the matching sentinel ADDRESS, `x === null` against NULL.
    if a_nullish ^ b_nullish {
        let (val, nullish_is_undef) = if a_nullish {
            (b, a_undef)
        } else {
            (a, b_undef)
        };
        let val_ty = ctx.operand_ty(&val);
        if !matches!(val_ty, Type::Str | Type::Substr) {
            return None;
        }
        let rhs = if nullish_is_undef {
            let (fid, sentinel_ty) = if val_ty == Type::Str {
                (ctx.intrinsics.str_undef, Type::Str)
            } else {
                (ctx.intrinsics.substr_undef, Type::Substr)
            };
            let u = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(fid, vec![]),
                sentinel_ty,
                None,
            );
            Operand::Value(u)
        } else {
            Operand::ConstPtrNull
        };
        let cmp = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::ICmp(IPred::Eq, val, rhs),
            Type::Bool,
            None,
        );
        if matches!(op, AstBinOp::Neq) {
            let r = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::BinOp(SsaBinOp::Xor, Operand::Value(cmp), Operand::ConstBool(true)),
                Type::Bool,
                None,
            );
            return Some(Operand::Value(r));
        }
        return Some(Operand::Value(cmp));
    }
    None
}
