//! `Symbol(desc?)` direct constructor call pulled out of
//! [`crate::ssa_lower::lower_expr_inner`] `Expr::Call` dispatch as
//! chunk-39 of the `Expr::Call` god-arm decomp (chunks 1-38 = ... +
//! namespace-obj method dispatch).
//!
//! T-13.a (v0.4.0) — `Symbol(desc?)` constructor (call, not `new`).
//! Returns `Type::Symbol`. `desc` is optional; missing argument lowers
//! to `Operand::ConstPtrNull` (the `__torajs_symbol_alloc` runtime
//! treats a NULL desc as no-description — `rc_inc` no-ops and `print`
//! formats as `Symbol()`).
//!
//! S308 — trailing args[1..] lowered-and-dropped per S272 idiom so
//! step()-style side-effect exprs fire per ES §20.4.1 trailing-arg
//! ignore (check.rs S308 already typecheck-dropped).
//!
//! Returns `Some(op)` on hit; `None` on miss (callee not
//! `Ident("Symbol")`). Distinct from the `Symbol.for(key)` global-
//! registry path (that one routes through namespace dispatch).

use crate::ast::{Expr, ExprId};
use crate::check::Type as CheckType;
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
    if n != "Symbol" {
        return None;
    }
    // §20.4.1.1 step 2 — an `undefined` description means no
    // description. Evaluate the arg for its side effects, then feed
    // NULL (the no-desc form) instead of the undefined value.
    let arg0_undef =
        !args.is_empty() && matches!(ctx.expr_types.get(&args[0]), Some(CheckType::Undefined));
    let desc_op: Operand = if args.is_empty() {
        Operand::ConstPtrNull
    } else if arg0_undef {
        let _ = ctx.lower_expr(args[0]);
        Operand::ConstPtrNull
    } else {
        ctx.lower_expr(args[0])
    };
    // S308 — lower-and-drop trailing args[1..] per S272 idiom so
    // step()-style side-effect exprs fire per ES §20.4.1 trailing-
    // arg ignore (check.rs S308 already typecheck-dropped).
    for &a in args.iter().skip(1) {
        let _ = ctx.lower_expr(a);
    }
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.symbol_alloc, vec![desc_op.clone()]),
        Type::Symbol,
        None,
    );
    // `symbol_alloc` SHARES the desc (internal rc_inc + store, its own
    // drop dec's it), so an Ident desc keeps its stake and its scope
    // drop — the old consume path orphaned that stake (RFC 20260705
    // ledger #3, 32B/iter probe). An owned temp desc releases here.
    if !args.is_empty() && !arg0_undef {
        ctx.release_owned_temp(args[0], &desc_op);
    }
    Some(Operand::Value(v))
}
