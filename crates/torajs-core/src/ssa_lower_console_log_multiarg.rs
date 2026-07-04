//! Multi-arg `console.log(arg0, arg1, …)` per-arg inspect dispatch —
//! carved out of `ssa_lower.rs` so this trunk (B134) lives outside
//! the 27k-line god-file. Both `lower_top_stmt` and `lower_stmt`'s
//! `Stmt::Expr` arm route through [`try_lower`] before falling back
//! to the generic expression lowering; that pre-B134 fork left the
//! `lower_stmt` path on the legacy `console.error/warn` Str-coerce
//! joiner, which panics on typed `Arr<T>` args at
//! `coerce_to_str(Type::Arr)`. Centralising the path here fixes the
//! try-body case without re-deriving the dispatch table inline at
//! every caller.
//!
//! Per-arg shape:
//! * typed `Arr<i64/f64/bool/str/substr>` → matching no-`\n` typed
//!   walker (`__torajs_arr_print_<T>_inline`, `torajs-arr::print_inline`).
//!   Typed slots are raw bytes, not NaN-box, so the Any path's
//!   `Tag::Arr` arm would SIGSEGV.
//! * everything else → box to Any (`Type::Any` operand passes
//!   through unchanged) and call `__torajs_print_anyv_inline_top`
//!   (Str unquoted, no `\n`).
//!
//! Between args we emit a single `' '` via `__torajs_io_putc_stdout`,
//! and after the last arg we emit `'\n'` the same way.
//!
//! `console.error` / `console.warn` keep the legacy Str-coerce +
//! `str_concat` joiner path in `ssa_lower.rs` (less coverage in
//! conformance; not part of this trunk).

use crate::ast::{Expr, ExprId, Stmt};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// Try to lower `s` as a multi-arg `console.log` Stmt::Expr. Returns
/// `true` when handled — caller should treat the statement as fully
/// emitted; `false` falls through to the caller's default lowering.
pub(crate) fn try_lower(ctx: &mut LowerCtx, s: &Stmt) -> bool {
    let Stmt::Expr(eid) = s else {
        return false;
    };
    let Expr::Call { callee, args } = ctx.ast.get_expr(*eid) else {
        return false;
    };
    let Some(method) = ctx.console_method_member(*callee) else {
        return false;
    };
    if method != "log" || args.len() <= 1 {
        return false;
    }
    let arg_ids: Vec<ExprId> = args.clone();
    let space_op = Operand::ConstI64(b' ' as i64);
    let newline_op = Operand::ConstI64(b'\n' as i64);
    for (i, &aid) in arg_ids.iter().enumerate() {
        if i > 0 {
            ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.io_putc_stdout, vec![space_op.clone()]),
                Type::I64,
                None,
            );
        }
        // S139 — `undefined` typed-expr prints "undefined" inline.
        // Multi-arg path's box_to_any → print_any_inline_top correctly
        // handles Type::Null (tag = ANY_NULL → "null") but Undefined
        // lowers to ConstPtrNull at the operand layer with no
        // Type::Undefined sentinel, so the tag-aware printer falls
        // back to ANY_NULL too. Detect Type::Undefined via expr_types
        // and box a literal "undefined" string so the printer's
        // ANY_STR arm picks it up unquoted. Catches Expr::Ident("
        // undefined") AND S138 derived `undefined && x` style.
        if matches!(
            ctx.expr_types.get(&aid),
            Some(crate::check::Type::Undefined)
        ) {
            let _ = ctx.lower_expr(aid);
            let lit = ctx.intern_string_literal("undefined");
            let boxed = ctx.box_to_any(Operand::Value(lit));
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.print_any_inline_top, vec![boxed.clone()]),
            );
            ctx.emit_drop_value(boxed, Type::Any);
            continue;
        }
        // Same borrow judgement as `lower_single_arg`: Ident / Member
        // lower to a borrowed operand the local (or its container)
        // still owns; everything else (literal / call / new) is a
        // temp this statement owns.
        let is_borrow = matches!(ctx.ast.get_expr(aid), Expr::Ident(_) | Expr::Member { .. });
        let arg = ctx.lower_expr(aid);
        let arg_ty = ctx.operand_ty(&arg);

        // typed Arr<primitive> — route to the matching no-\n typed
        // walker (slots are raw bytes, not NaN-box, so the Any path's
        // Tag::Arr arm would SIGSEGV).
        if let Type::Arr(arr_id) = arg_ty {
            let elem_ty = ctx.arr_layouts[arr_id.0 as usize];
            let typed_target = match elem_ty {
                Type::I64 => Some(ctx.intrinsics.arr_print_i64_inline),
                Type::F64 => Some(ctx.intrinsics.arr_print_f64_inline),
                Type::Bool => Some(ctx.intrinsics.arr_print_bool_inline),
                Type::Str => Some(ctx.intrinsics.arr_print_str_inline),
                Type::Substr => Some(ctx.intrinsics.arr_print_substr_inline),
                _ => None,
            };
            if let Some(target) = typed_target {
                ctx.f
                    .append_void(ctx.cur_block, InstKind::Call(target, vec![arg]));
                continue;
            }
        }

        // Everything else: box to Any (a Type::Any operand passes
        // through unchanged), print via the tag-aware no-\n entry,
        // then drop the box. `box_to_any` is TRANSFER for refcounted
        // values (`anyv_box_from_pair` tag=4 takes ownership of an
        // rc, no inc) — a borrowed operand must be inc'd first so
        // the post-print drop releases the box's reference, not the
        // owner's (RFC 20260704 S6: pre-fix this net-negative dec
        // was masked by the anyv underflow leak; with hit-zero
        // actually freeing, printing a live local freed it).
        let (any_op, drop_after) = if arg_ty == Type::Any {
            (arg, false)
        } else {
            if is_borrow && arg_ty.is_refcounted() {
                ctx.emit_rc_inc(arg.clone());
            }
            (ctx.box_to_any(arg), true)
        };
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.print_any_inline_top, vec![any_op.clone()]),
        );
        if drop_after {
            ctx.emit_drop_value(any_op, Type::Any);
        }
    }
    ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.io_putc_stdout, vec![newline_op]),
        Type::I64,
        None,
    );
    true
}
