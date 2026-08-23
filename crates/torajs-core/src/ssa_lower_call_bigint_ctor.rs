//! `BigInt(value)` callable ctor pulled out of
//! [`crate::ssa_lower::lower_expr_inner`] `Expr::Call` dispatch as
//! chunk-37 of the `Expr::Call` god-arm decomp (chunks 1-36 = ... +
//! Arr.{keys|values|entries}() ArrIter ctor).
//!
//! V3-03 — `BigInt(value)` callable. Single required arg; dispatch
//! on the arg's static SSA type:
//!   - `Type::BigInt` → `bigint_clone`
//!   - `Type::Str`    → `bigint_from_str`
//!   - `Type::F64`    → `bigint_from_number`
//!   - `Type::I64`    → SiToFp → `bigint_from_number` (the helper
//!                      rejects non-integer + non-finite Numbers with
//!                      RangeError per spec)
//!
//! S308 — trailing args[1..] lowered-and-dropped per S272 idiom so
//! step()-style side-effect exprs fire per ES §21.2.1 trailing-arg
//! ignore (check.rs S308 already typecheck-dropped).
//!
//! `Type::Any` deferred — Any-tagged dispatch is a follow-up; the
//! typechecker accepts the three concrete types above and routes
//! `Type::Any` through the generic resolve_callee path.
//!
//! Returns `Some(op)` on hit; `None` on miss (callee not
//! `Ident("BigInt")` or args empty).

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let Expr::Ident(n) = ctx.ast.get_expr(callee) else {
        return None;
    };
    if n != "BigInt" || args.is_empty() {
        return None;
    }
    let arg_op = ctx.lower_expr(args[0]);
    let arg_ty = ctx.operand_ty(&arg_op);
    // S308 — lower-and-drop trailing args[1..] per S272 idiom so
    // step()-style side-effect exprs fire per ES §21.2.1 trailing-
    // arg ignore (check.rs S308 already typecheck-dropped).
    for &a in args.iter().skip(1) {
        let _ = ctx.lower_expr(a);
    }
    let cur_block = ctx.cur_block;
    let v = match arg_ty {
        Type::BigInt => ctx.f.append_inst(
            cur_block,
            InstKind::Call(ctx.intrinsics.bigint_clone, vec![arg_op.clone()]),
            Type::BigInt,
            None,
        ),
        Type::Str => ctx.f.append_inst(
            cur_block,
            InstKind::Call(ctx.intrinsics.bigint_from_str, vec![arg_op.clone()]),
            Type::BigInt,
            None,
        ),
        Type::F64 => ctx.f.append_inst(
            cur_block,
            InstKind::Call(ctx.intrinsics.bigint_from_number, vec![arg_op.clone()]),
            Type::BigInt,
            None,
        ),
        Type::I64 => {
            let f = ctx
                .f
                .append_inst(cur_block, InstKind::SiToFp(arg_op.clone()), Type::F64, None);
            ctx.f.append_inst(
                cur_block,
                InstKind::Call(ctx.intrinsics.bigint_from_number, vec![Operand::Value(f)]),
                Type::BigInt,
                None,
            )
        }
        // §21.2.1.1 over an Any operand — ToPrimitive first, a
        // Number primitive through NumberToBigInt, the rest through
        // the §7.1.13 dispatch. The kernel records its own throws.
        Type::Any => ctx.f.append_inst(
            cur_block,
            InstKind::Call(ctx.intrinsics.bigint_ctor_any, vec![arg_op.clone()]),
            Type::BigInt,
            None,
        ),
        _ => panic!("ssa-lower: BigInt() expects bigint / string / number arg, got {arg_ty:?}"),
    };
    // `bigint_from_str` / `bigint_clone` borrow their arg (pure reads
    // into a fresh BigInt), so an Ident arg keeps its stake and its
    // scope drop — the old consume path orphaned that stake (RFC
    // 20260705 ledger #3, 32B/iter probe). Owned temps release here.
    ctx.release_owned_temp(args[0], &arg_op);
    // from_str's parse-failure SyntaxError (RFC 20260720 刀 5b) and
    // from_number's non-integer RangeError propagate to user
    // try/catch here.
    ctx.emit_throw_check(None);
    Some(Operand::Value(v))
}
