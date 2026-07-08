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
    // Chunk 570 — container Index reads are borrows too (the slot
    // owns the elem; dropping the read stole the slot's stake and
    // the array's death then freed the source — UAF, probe-proven).
    // String indexing stays owned: `s[i]` mints a fresh Substr view
    // (chunk 561 family predicate). OptChain / This align with
    // expr_is_fresh_owned's borrow set.
    let is_borrow = match ctx.ast.get_expr(arg_id) {
        Expr::Ident(_) | Expr::Member { .. } | Expr::OptChain { .. } | Expr::This => true,
        Expr::Index { obj, .. } => {
            !matches!(ctx.expr_types.get(obj), Some(crate::check::Type::String))
        }
        _ => false,
    };
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
    // RFC 20260708-typed-arr-oob-read chunk 2 — an F64 read off a
    // number[] index may hold the undefined-NaN sentinel; branch to
    // the Str printer with the immortal sentinel cell (payload
    // "undefined") instead of printing the raw NaN.
    if arg_ty == Type::F64 && crate::ssa_lower_nullable_guard::is_undef_f64_source(ctx, arg_id) {
        return lower_print_f64_or_undef(ctx, method, arg);
    }
    let is_str = arg_ty == Type::Str;
    let target = ctx.console_print_target(method, arg_ty);
    // RFC 20260704 L3b #5 — a typed Arr whose elem has no dedicated
    // typed printer (Arr<Arr> / Arr<Obj> / …) routes through the
    // tag-aware print_any, which reads the header's elem-kind field.
    // This direct typed path never crosses the typed→Any boxing
    // boundary, so mark the kind chain here; unmarked (UNSET) the
    // walker reads raw i64 slots as NaN-box values and dereferences
    // small ints as cell pointers (SIGSEGV).
    if target == ctx.intrinsics.print_any && matches!(arg_ty, Type::Arr(_)) {
        ctx.emit_arr_mark_kind(&arg);
    }
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

/// RFC 20260708-typed-arr-oob-read chunk 2 — two-state print for a
/// possibly-sentinel F64: the undefined branch prints the immortal
/// Str sentinel (payload "undefined", FLAG_STATIC_LITERAL → no rc
/// traffic), the number branch takes the plain F64 printer.
pub(crate) fn lower_print_f64_or_undef(
    ctx: &mut LowerCtx<'_>,
    method: &'static str,
    arg: Operand,
) -> Operand {
    use crate::ssa::{IPred, Terminator};
    let bits = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::BitCastF64ToI64(arg.clone()),
        Type::I64,
        None,
    );
    let is_undef = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(
            IPred::Eq,
            Operand::Value(bits),
            Operand::ConstI64(crate::ssa_lower_nullable_guard::F64_UNDEF_SENTINEL_BITS as i64),
        ),
        Type::Bool,
        None,
    );
    let undef_blk = ctx.f.add_block();
    let num_blk = ctx.f.add_block();
    let merge = ctx.f.add_block();
    let cb = ctx.cur_block;
    ctx.f.set_term(
        cb,
        Terminator::CondBr {
            cond: Operand::Value(is_undef),
            then_blk: undef_blk,
            else_blk: num_blk,
        },
    );
    ctx.cur_block = undef_blk;
    let sentinel = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::GlobalRef(crate::ssa_lower_intrinsics_str_b::STR_UNDEF_CELL_SYM.to_string()),
        Type::Str,
        None,
    );
    let str_target = ctx.console_print_target(method, Type::Str);
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Call(str_target, vec![Operand::Value(sentinel)]),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(merge));
    ctx.cur_block = num_blk;
    let f64_target = ctx.console_print_target(method, Type::F64);
    ctx.f
        .append_void(ctx.cur_block, InstKind::Call(f64_target, vec![arg]));
    ctx.f.set_term(ctx.cur_block, Terminator::Br(merge));
    ctx.cur_block = merge;
    Operand::ConstI64(0)
}
