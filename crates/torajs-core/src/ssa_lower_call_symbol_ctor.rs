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
    // §20.4.1.1 step 3 — every other description is `? ToString(d)`,
    // NOT a string-shaped operand: `Symbol(1)` describes itself "1".
    // The IMPLICIT ToString face is the right one (§7.1.17) — a Symbol
    // description throws TypeError there, which is what
    // `built-ins/Symbol/desc-to-string-symbol.js` asserts; the display
    // face would have answered "Symbol(1)" instead. It also buys the
    // OrdinaryToPrimitive walk that `desc-to-string.js` counts
    // (toString-then-valueOf, each hook's throw propagating).
    //
    // The result is always OWNED — the Str arm rc_inc's a borrow-shaped
    // source before passing it through, every other arm mints fresh —
    // so the release below is unconditional rather than the old
    // owned-temp-only test.
    let desc_op: Operand = if args.is_empty() {
        Operand::ConstPtrNull
    } else if arg0_undef {
        let _ = ctx.lower_expr(args[0]);
        Operand::ConstPtrNull
    } else {
        let raw = ctx.lower_expr(args[0]);
        let raw_ty = ctx.operand_ty(&raw);
        let s = crate::ssa_lower_call_coercion::emit_to_string(ctx, args[0], raw, raw_ty, true);
        // A throwing ToString must not reach `symbol_alloc` — the
        // Any/Symbol arms record the pending throw and return a
        // placeholder, so the check has to land before the mint.
        ctx.emit_throw_check(None);
        s
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
    // drop dec's it), so the ToString result — always owned, see above
    // — releases its own stake here. Pre-ToString this was an
    // owned-temp-only release because a bare Ident desc was passed
    // through as a borrow and had to keep the binding's stake (RFC
    // 20260705 ledger #3, 32B/iter probe); the rc_inc inside the Str
    // arm is what makes the unconditional form correct now.
    if !args.is_empty() && !arg0_undef {
        let desc_ty = ctx.operand_ty(&desc_op);
        if !desc_ty.is_copy() {
            ctx.emit_drop_value(desc_op.clone(), desc_ty);
        }
    }
    Some(Operand::Value(v))
}
