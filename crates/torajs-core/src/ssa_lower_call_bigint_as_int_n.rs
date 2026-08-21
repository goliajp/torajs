//! P12.4-B/C — `BigInt.asIntN(bits, value)` / `BigInt.asUintN(bits,
//! value)` per ES §21.2.2.{1,2} pulled out of
//! [`crate::ssa_lower::lower_expr_inner`]'s `Expr::Call` god-arm as
//! chunk-51 of the decomp (chunks 1-50 = ... + Math.min/max
//! variadic).
//!
//! The runtime path supports arbitrary `bits >= 0` (multi-limb
//! masking); negative bits and the asUintN negative-input size cap
//! route through `__torajs_throw_range_error` inside the intrinsic
//! and the `emit_throw_check(None)` post-call propagates the pending
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
    // §21.2.2.{1,2} step 1 is `ToIndex(bits)`, which is a coercion
    // followed by a range test — so every operand shape reaches it,
    // and the test runs on the Number rather than on whatever an
    // early truncation left behind. The old `FpToSi` did neither:
    // `asIntN(Infinity, 0n)` and `asIntN(2**53, 0n)` should be
    // RangeErrors and were not, and `asIntN(NaN, 255n)` should be
    // `0n` (ToIndex(NaN) = 0) and answered `255n`.
    let bits_num = ctx.lower_to_number_operand(args[0]);
    let bits_f64 = ctx.coerce_to_f64(bits_num);
    let cur_block = ctx.cur_block;
    let bits_i64 = Operand::Value(ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.num_to_index, vec![bits_f64]),
        Type::I64,
        None,
    ));
    // §13.3.6.1 evaluates every argument before the call, and step 1
    // runs after that — so the RangeError propagates here, between
    // the operands and the kernel.
    ctx.emit_throw_check(None);
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
