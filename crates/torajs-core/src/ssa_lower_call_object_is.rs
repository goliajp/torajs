//! `Object.is(a, b)` SameValue dispatch pulled out of
//! [`crate::ssa_lower::lower_expr_inner`] `Expr::Call` dispatch as
//! chunk-29 of the `Expr::Call` god-arm decomp (chunks 1-28 = ... +
//! Object.* integrity/meta cluster).
//!
//! v0.2 #3 — `Object.is(a, b)` per ES §20.1.2.9 SameValue. Dispatches
//! by arg SSA type:
//!
//!   - `Type::F64`         → `__torajs_object_is_f64`
//!     (NaN/NaN → `true`, +0/-0 → `false`; bit-level compare per spec)
//!   - `Type::Str`         → `__torajs_str_eq` (value compare)
//!   - `Type::I64` / `Bool` / heap pointer → `ICmp Eq` directly
//!     (same memory representation, so equality is identity)
//!
//! Mismatched-type args (e.g. `Object.is("1", 1)`) short-circuit to
//! `ConstBool(false)` since `===` says so.
//!
//! **Borrow detection (v0.4.0 fix)**: if an arg is an
//! `Ident` / `Member` / `Index` expression, the operand returned by
//! `lower_expr` is a **borrow** (the array / object slot still owns
//! the rc). Calling `emit_drop_value` on it would rc_dec the owner's
//! ref → eventually frees a still-live element → SIGSEGV on the next
//! access. So drop only fresh values (call results, literals, etc.).
//! Mirrors the borrow guard used by the console.log path below.
//! test262 staging/sm/Symbol/equality.js covered this — `Object.is`
//! on two indexed Symbol-array reads inside a loop crashed after the
//! first iter because the array's element rc was being dec'd by
//! `Object.is`.
//!
//! **Trailing args (S306)**: lower-and-drop `args[2..]` per S272 idiom
//! so step()-style side-effect exprs fire per ES §20.1.2.9 trailing-
//! arg ignore (check.rs S257 already typecheck-dropped).
//!
//! Returns `Some(op)` on hit; `None` on miss (non-Member callee,
//! non-`Object.is`, or `args.len() < 2`) so the caller falls through.

use crate::ast::{Expr, ExprId};
use crate::ssa::{IPred, InstKind, Operand, Type};
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
    let recv_id = *ns_id;
    if m_name != "is" {
        return None;
    }
    let Expr::Ident(ns) = ctx.ast.get_expr(recv_id) else {
        return None;
    };
    if ns != "Object" {
        return None;
    }
    // S257 — widen `== 2` → `>= 2` per ES §20.1.2.9 trailing-arg
    // ignore. Lower args[0..2]; trailing dropped at lower-time.
    if args.len() < 2 {
        return None;
    }

    let a_borrow = matches!(
        ctx.ast.get_expr(args[0]),
        Expr::Ident(_) | Expr::Member { .. } | Expr::Index { .. }
    );
    let b_borrow = matches!(
        ctx.ast.get_expr(args[1]),
        Expr::Ident(_) | Expr::Member { .. } | Expr::Index { .. }
    );
    let a_op = ctx.lower_expr(args[0]);
    let b_op = ctx.lower_expr(args[1]);
    for &a in args.iter().skip(2) {
        let _ = ctx.lower_expr(a);
    }
    let a_ty = ctx.operand_ty(&a_op);
    let b_ty = ctx.operand_ty(&b_op);
    if a_ty != b_ty {
        // JS Number is ONE type — the I64/F64 split is a torajs
        // internal representation detail, so a numeric cross-tag
        // pair compares by value through the f64 SameValue kernel
        // (an i64-held value is always f64-exact under JS Number
        // semantics; the SiToFp widen is lossless there).
        if matches!(
            (a_ty, b_ty),
            (Type::I64, Type::F64) | (Type::F64, Type::I64)
        ) {
            let cur_block = ctx.cur_block;
            let widen = |ctx: &mut LowerCtx<'_>, op: Operand, ty: Type| -> Operand {
                if ty == Type::I64 {
                    Operand::Value(ctx.f.append_inst(
                        cur_block,
                        InstKind::SiToFp(op),
                        Type::F64,
                        None,
                    ))
                } else {
                    op
                }
            };
            let a_f = widen(ctx, a_op, a_ty);
            let b_f = widen(ctx, b_op, b_ty);
            let r = ctx.f.append_inst(
                cur_block,
                InstKind::Call(ctx.intrinsics.object_is_f64, vec![a_f, b_f]),
                Type::Bool,
                None,
            );
            return Some(Operand::Value(r));
        }
        // === yields false on differing types; same for Object.is.
        // Drop only fresh args; borrows stay owned by their source.
        if !a_borrow {
            ctx.emit_drop_value(a_op.clone(), a_ty);
        }
        if !b_borrow {
            ctx.emit_drop_value(b_op, b_ty);
        }
        return Some(Operand::ConstBool(false));
    }

    let cur_block = ctx.cur_block;
    let result_v = match a_ty {
        Type::F64 => ctx.f.append_inst(
            cur_block,
            InstKind::Call(
                ctx.intrinsics.object_is_f64,
                vec![a_op.clone(), b_op.clone()],
            ),
            Type::Bool,
            None,
        ),
        Type::Str => ctx.f.append_inst(
            cur_block,
            InstKind::Call(ctx.intrinsics.str_eq, vec![a_op.clone(), b_op.clone()]),
            Type::Bool,
            None,
        ),
        _ => ctx.f.append_inst(
            cur_block,
            InstKind::ICmp(IPred::Eq, a_op.clone(), b_op.clone()),
            Type::Bool,
            None,
        ),
    };
    if !a_borrow {
        ctx.emit_drop_value(a_op, a_ty);
    }
    if !b_borrow {
        ctx.emit_drop_value(b_op, b_ty);
    }
    Some(Operand::Value(result_v))
}
