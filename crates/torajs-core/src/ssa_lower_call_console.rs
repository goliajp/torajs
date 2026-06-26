//! `console.<method>(...)` dispatch (single-arg + multi-arg) pulled
//! out of [`crate::ssa_lower::lower_expr_inner`] `Expr::Call`
//! dispatch as chunk-30 of the `Expr::Call` god-arm decomp (chunks
//! 1-29 = ... + Object.is SameValue).
//!
//! Two arms share the `console.<m>` receiver shape:
//!
//! - **Single-arg path** — typed-Str / Substr / primitive value goes
//!   straight to the type-specific `__torajs_console_<m>_<ty>` print
//!   target. Substr gets one rc bump via `substr_to_owned` so the
//!   print helper sees a normal Str (slot still owned by the source).
//!   Borrow detection on `Ident` / `Member` args avoids rc_dec'ing
//!   the source's ref after print (mirrors the chunk-29 `Object.is`
//!   guard; same SIGSEGV class).
//! - **Multi-arg path** — coerce each arg to Str, join with `" "`,
//!   print once via the Str-typed target. Duplicate of the multi-arg
//!   joiner already in `lower_top_stmt`; this is the in-expr /
//!   inside-fn-body variant where the same machinery has to run on
//!   the SSA emit side rather than the statement-level fast path.
//!
//! Both arms return `Some(Operand::ConstI64(0))` (console.* returns
//! `undefined`). Returns `None` on miss (non-console receiver, or 0
//! args) so the caller falls through to subsequent arms.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let method = ctx.console_method_member(callee)?;
    if args.is_empty() {
        return None;
    }
    if args.len() == 1 {
        Some(lower_single_arg(ctx, method, args[0]))
    } else {
        Some(lower_multi_arg(ctx, method, args))
    }
}

/// `console.<m>(v)` — type-specific print target. Substr gets a
/// one-time own copy; primitives and Str pass straight through.
fn lower_single_arg(ctx: &mut LowerCtx<'_>, method: &'static str, arg_id: ExprId) -> Operand {
    let is_borrow = matches!(
        ctx.ast.get_expr(arg_id),
        Expr::Ident(_) | Expr::Member { .. }
    );
    let arg = ctx.lower_expr(arg_id);
    let arg_ty = ctx.operand_ty(&arg);
    let cur_block = ctx.cur_block;
    if arg_ty == Type::Substr {
        let substr_to_owned = ctx.intrinsics.substr_to_owned;
        let owned = ctx.f.append_inst(
            cur_block,
            InstKind::Call(substr_to_owned, vec![arg.clone()]),
            Type::Str,
            None,
        );
        let target = ctx.console_print_target(method, Type::Str);
        ctx.f.append_void(
            cur_block,
            InstKind::Call(target, vec![Operand::Value(owned)]),
        );
        ctx.emit_drop_value(Operand::Value(owned), Type::Str);
        if !is_borrow {
            ctx.emit_drop_value(arg, Type::Substr);
        }
        return Operand::ConstI64(0);
    }
    let is_str = arg_ty == Type::Str;
    let target = ctx.console_print_target(method, arg_ty);
    ctx.f
        .append_void(cur_block, InstKind::Call(target, vec![arg.clone()]));
    if is_str && !is_borrow {
        ctx.emit_drop_value(arg, Type::Str);
    }
    Operand::ConstI64(0)
}

/// `console.<m>(a, b, ...)` — coerce each to Str, join with `" "`,
/// print the joined Str via the Str-typed target.
fn lower_multi_arg(ctx: &mut LowerCtx<'_>, method: &'static str, args: &[ExprId]) -> Operand {
    let space_str = ctx.intern_string_literal(" ");
    let mut acc: Option<Operand> = None;
    for (i, &aid) in args.iter().enumerate() {
        let arg = ctx.lower_expr(aid);
        let arg_ty = ctx.operand_ty(&arg);
        let s_op = ctx.coerce_to_str(arg, arg_ty);
        if i > 0 {
            let prev = acc.unwrap();
            let str_concat = ctx.intrinsics.str_concat;
            let cur_block = ctx.cur_block;
            let with_sep = ctx.f.append_inst(
                cur_block,
                InstKind::Call(str_concat, vec![prev, Operand::Value(space_str)]),
                Type::Str,
                None,
            );
            let combined = ctx.f.append_inst(
                cur_block,
                InstKind::Call(str_concat, vec![Operand::Value(with_sep), s_op]),
                Type::Str,
                None,
            );
            acc = Some(Operand::Value(combined));
        } else {
            acc = Some(s_op);
        }
    }
    let target = ctx.console_print_target(method, Type::Str);
    let final_str = acc.unwrap();
    let cur_block = ctx.cur_block;
    ctx.f
        .append_void(cur_block, InstKind::Call(target, vec![final_str.clone()]));
    ctx.emit_drop_value(final_str, Type::Str);
    Operand::ConstI64(0)
}
