//! P12.4-B/C — `BigInt.asIntN(bits, value)` / `BigInt.asUintN(bits,
//! value)` per ES §21.2.2.{1,2} pulled out of
//! [`crate::ssa_lower::lower_expr_inner`]'s `Expr::Call` god-arm as
//! chunk-51 of the decomp (chunks 1-50 = ... + Math.min/max
//! variadic).
//!
//! The runtime path supports `bits in [0, 64]`; larger bits route
//! through `__torajs_throw_range_error` inside the intrinsic and
//! the `emit_throw_check(None)` post-call propagates the pending
//! throw to user-visible `try/catch`.
//!
//! - bits arg coerced to `Type::I64`: F64 → `FpToSi`; other types
//!   pass through (the runtime helper validates).
//! - **S314** — ES §21.2.2.{1,2} silently ignore trailing args past
//!   `(bits, value)`. Trailing args still get `lower_expr`'d (S272
//!   idiom) so step()-style side-effect exprs fire left-to-right.
//! - args share: the helper borrows the value BigInt; owned temps
//!   release post-call (RFC 20260705 ledger #3).
//!
//! Returns `Some(op)` on hit; `None` on miss (callee not the
//! `BigInt.{asIntN|asUintN}` Member-Ident shape, or args.len < 2).

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    if args.len() < 2 {
        return None;
    }
    let Expr::Member {
        obj: ns_id,
        name: m_name,
    } = ctx.ast.get_expr(callee)
    else {
        return None;
    };
    let is_int_n = m_name == "asIntN";
    if !is_int_n && m_name != "asUintN" {
        return None;
    }
    let ns_id = *ns_id;
    let Expr::Ident(ns) = ctx.ast.get_expr(ns_id) else {
        return None;
    };
    if ns != "BigInt" {
        return None;
    }
    let bits_op = ctx.lower_expr(args[0]);
    let bits_ty = ctx.operand_ty(&bits_op);
    let bits_i64 = match bits_ty {
        Type::I64 => bits_op,
        Type::F64 => {
            let cur_block = ctx.cur_block;
            Operand::Value(
                ctx.f
                    .append_inst(cur_block, InstKind::FpToSi(bits_op), Type::I64, None),
            )
        }
        _ => bits_op,
    };
    let val_op = ctx.lower_expr(args[1]);
    for &a in args.iter().skip(2) {
        let _ = ctx.lower_expr(a);
    }
    let target = if is_int_n {
        ctx.intrinsics.bigint_as_int_n
    } else {
        ctx.intrinsics.bigint_as_uint_n
    };
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(target, vec![bits_i64, val_op.clone()]),
        Type::BigInt,
        None,
    );
    // The helper borrows the value BigInt (reads words into a fresh
    // result), so an Ident arg keeps its stake and its scope drop —
    // the old consume path orphaned that stake (RFC 20260705 ledger
    // #3). Owned temps release here.
    ctx.release_owned_temp(args[1], &val_op);
    ctx.emit_throw_check(None);
    Some(Operand::Value(v))
}
