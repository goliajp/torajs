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
//! Chunk 808 — two phases, matching ES argument-evaluation order:
//! phase 1 lowers EVERY argument (all side effects run before any
//! output), phase 2 prints. The old streaming loop interleaved a
//! side-effecting argument's output into the log line
//! (`console.log(1, s("a"))` printed `1 a\n9` where bun prints
//! `a\n1 9`). Refcounted borrows rc_inc in phase 1 — that both
//! keeps the printed value alive across later arguments' side
//! effects AND pins the ES semantics of printing the value as it
//! was when evaluated (a later arg reassigning the binding must
//! not change what prints).
//!
//! Per-arg print shape (phase 2):
//! * typed `Arr<i64/f64/bool/str/substr>` → matching no-`\n` typed
//!   walker (`__torajs_arr_print_<T>_inline`, `torajs-arr::print_inline`).
//!   Typed slots are raw bytes, not NaN-box, so the Any path's
//!   `Tag::Arr` arm would SIGSEGV.
//! * `FnSig` → fnname ToString + Str-box print.
//! * everything else → box to Any (`Type::Any` operand passes
//!   through unchanged) and call `__torajs_print_anyv_inline_top`
//!   (Str unquoted, no `\n`).
//!
//! Between args we emit a single `' '` via `__torajs_io_putc_out`,
//! and after the last arg we emit `'\n'` the same way.
//!
//! `console.error` / `console.warn` keep the legacy Str-coerce +
//! `str_concat` joiner path in `ssa_lower.rs` (less coverage in
//! conformance; not part of this trunk).

use crate::ast::{Expr, ExprId, Stmt};
use crate::ssa::{InstKind, Operand, Terminator, Type};
use crate::ssa_lower::LowerCtx;

/// One evaluated argument, carried from phase 1 to phase 2.
struct LoweredArg {
    aid: ExprId,
    /// `None` — S139 static-undefined shape (checker typed the expr
    /// Undefined and it is not a value we print from an operand; the
    /// effect already ran in phase 1, phase 2 prints the literal).
    op: Option<Operand>,
    ty: Type,
    /// The refcounted stake phase 1 minted for this lane (borrow
    /// rc_inc / Any retain / owned Any temp) — phase 2 releases it
    /// after printing. Fresh non-Any temps carry their own stake
    /// that the box transfer consumes, so they are NOT staked here.
    staked: bool,
    /// r505 — a refcounted arg the expression minted (not a borrow):
    /// the typed-inline arm releases it after the print, as the
    /// any-box arm's drop of the box always did.
    owned: bool,
}

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

    // Phase 1 — evaluate every argument in source order.
    let mut lowered: Vec<LoweredArg> = Vec::with_capacity(arg_ids.len());
    for &aid in &arg_ids {
        // S139 — `undefined`-typed exprs print the literal; a
        // call-shaped one (void call) still runs for effect here.
        // Catches Expr::Ident("undefined") AND S138 derived
        // `undefined && x` style.
        if matches!(
            ctx.expr_types.get(&aid),
            Some(crate::check::Type::Undefined)
        ) {
            let _ = ctx.lower_expr(aid);
            lowered.push(LoweredArg {
                aid,
                op: None,
                ty: Type::Void,
                staked: false,
                owned: false,
            });
            continue;
        }
        let mut arg = ctx.lower_expr(aid);
        let arg_ty = ctx.operand_ty(&arg);
        // Shared borrow judgement with `lower_single_arg`
        // (`console_arg_is_borrow`): Ident / This / Member /
        // OptChain / container Index lower to borrowed operands the
        // local (or its container) still owns; everything else
        // (literal / call / new / string index) is a temp this
        // statement owns. The judgement runs AFTER the lowering so
        // `owned_member_reads`-recorded reads answer owned (their
        // box transfer consumes the read's stake).
        let is_borrow = crate::ssa_lower_call_console::console_arg_is_borrow(ctx, aid);
        let staked = if arg_ty == Type::Any {
            // Chunk 721 — an owned Any temp (Call / OptCall results,
            // recorded member reads: `expr_owned_shape`) releases
            // after the print. Chunk 808 — a borrowed Any operand
            // takes its own stake too (`anyv_retain`; immediates
            // retain as no-ops and the paired drop no-ops): a later
            // argument's side effect may reassign the binding and
            // drop the cell this lane is about to print.
            if !ctx.expr_owned_shape(aid) {
                let retained = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Call(ctx.intrinsics.anyv_retain, vec![arg.clone()]),
                    Type::Any,
                    None,
                );
                arg = Operand::Value(retained);
            }
            true
        } else if arg_ty.is_refcounted() && is_borrow {
            // Phase-1 inc: keeps the value alive across later args'
            // side effects and pins print-as-evaluated semantics.
            ctx.emit_rc_inc(arg.clone());
            true
        } else {
            // Fresh refcounted temps carry their own stake (the box
            // transfer settles it in phase 2, matching the pre-808
            // ledger); Copy-typed values have no ledger.
            false
        };
        lowered.push(LoweredArg {
            aid,
            op: Some(arg),
            ty: arg_ty,
            staked,
            owned: arg_ty != Type::Any && arg_ty.is_refcounted() && !is_borrow,
        });
    }

    // Phase 2 — print.
    let space_op = Operand::ConstI64(b' ' as i64);
    let newline_op = Operand::ConstI64(b'\n' as i64);
    for (i, la) in lowered.into_iter().enumerate() {
        if i > 0 {
            ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.io_putc_out, vec![space_op.clone()]),
                Type::I64,
                None,
            );
        }
        print_one(ctx, la);
    }
    ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.io_putc_out, vec![newline_op]),
        Type::I64,
        None,
    );
    true
}

/// Phase-2 print of one evaluated argument (dispatch preserved from
/// the pre-808 streaming loop, minus the lowering).
fn print_one(ctx: &mut LowerCtx, la: LoweredArg) {
    let LoweredArg {
        aid,
        op,
        ty,
        staked,
        owned,
    } = la;
    let Some(arg) = op else {
        // S139 static-undefined shape — box a literal "undefined"
        // string so the printer's ANY_STR arm picks it up unquoted.
        let lit = ctx.intern_string_literal("undefined");
        let boxed = ctx.box_to_any(Operand::Value(lit));
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.print_any_inline_top, vec![boxed.clone()]),
        );
        ctx.emit_drop_value(boxed, Type::Any);
        return;
    };

    // RFC 20260710 C2b — an Obj/Arr/Closure arg may hold the
    // generic undefined cell (Nullable slot); branch to the
    // boxed "undefined" inline print (helper below), a live cell
    // falls through to the arms below unchanged.
    let sentinel_join = open_boxed_sentinel_branch(ctx, &arg, ty);

    // typed Arr<primitive> — route to the matching no-\n typed
    // walker (slots are raw bytes, not NaN-box, so the Any path's
    // Tag::Arr arm would SIGSEGV).
    if let Type::Arr(arr_id) = ty {
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
                .append_void(ctx.cur_block, InstKind::Call(target, vec![arg.clone()]));
            // Chunk 808 — release the phase-1 borrow inc; fresh
            // temps (not staked) keep the pre-808 ledger.
            if staked {
                ctx.emit_drop_value(arg, ty);
            }
            crate::ssa_lower_call_console::close_console_sentinel_branch(ctx, sentinel_join);
            return;
        }
    }

    // RFC 20260710 C2a — a fn-typed slot value has no box repr
    // (a code address is not a heap cell); ToString it through
    // the fnname runtime (null → "null", the undefined sentinel
    // → "undefined", a real address → "[Function: name]") and
    // print the owned Str via the Str-slot box.
    // r505 — a scalar / string argument prints through its own
    // newline-less printer, exactly the bytes the single-arg lane
    // would write for it. Boxing it into the any printer (the arm
    // below) rooted every inspectable world — 98 KB on
    // `console.log(1, 2)`. An F64 that may carry the undefined
    // sentinel keeps the two-state any box.
    let typed_inline = match ty {
        Type::I64 => Some(ctx.intrinsics.print_i64_inline),
        Type::Bool => Some(ctx.intrinsics.print_bool_inline),
        Type::F64 if !crate::ssa_lower_undef_f64_source::is_undef_f64_source(ctx, aid) => {
            Some(ctx.intrinsics.print_f64_inline)
        }
        Type::Str => Some(ctx.intrinsics.str_print_inline),
        Type::Substr => Some(ctx.intrinsics.substr_print_inline),
        _ => None,
    };
    if let Some(target) = typed_inline {
        ctx.f
            .append_void(ctx.cur_block, InstKind::Call(target, vec![arg.clone()]));
        if staked || owned {
            ctx.emit_drop_value(arg, ty);
        }
        crate::ssa_lower_call_console::close_console_sentinel_branch(ctx, sentinel_join);
        return;
    }
    if matches!(ty, Type::FnSig(_)) {
        let s = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.fnsig_to_str, vec![arg]),
            Type::Str,
            None,
        );
        let boxed = ctx.box_to_any(Operand::Value(s));
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.print_any_inline_top, vec![boxed.clone()]),
        );
        ctx.emit_drop_value(boxed, Type::Any);
        ctx.emit_drop_value(Operand::Value(s), Type::Str);
        crate::ssa_lower_call_console::close_console_sentinel_branch(ctx, sentinel_join);
        return;
    }

    // Everything else: box to Any (a Type::Any operand passes
    // through unchanged), print via the tag-aware no-\n entry,
    // then drop the box. `box_to_any` is TRANSFER for refcounted
    // values (`anyv_box_from_pair` tag=4 takes ownership of an
    // rc, no inc) — the phase-1 inc supplied the stake the box
    // consumes, so the post-print drop releases this lane's
    // reference, not the owner's (RFC 20260704 S6).
    let (any_op, drop_after) = if ty == Type::Any {
        (arg, staked)
    } else {
        // RFC 20260708-typed-arr-oob-read chunk 3 — a possibly-
        // sentinel F64 arg (number[] index read / alias) boxes
        // to ANY_UNDEF when the bits match so this prints
        // "undefined", not "NaN".
        let boxed = if ty == Type::F64
            && crate::ssa_lower_undef_f64_source::is_undef_f64_source(ctx, aid)
        {
            ctx.box_f64_or_undef(arg)
        } else {
            // RFC 20260710 C2b — expr-aware box: a Nullable
            // Obj/Arr/Closure arg may carry the generic
            // undefined cell, which must re-encode as ANY_UNDEF
            // (the tag-aware printer has no arm for the oddball
            // header). All other shapes take box_to_any's arms
            // unchanged.
            ctx.box_to_any_from_expr(aid, arg)
        };
        (boxed, true)
    };
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.print_any_inline_top, vec![any_op.clone()]),
    );
    if drop_after {
        ctx.emit_drop_value(any_op, Type::Any);
    }
    crate::ssa_lower_call_console::close_console_sentinel_branch(ctx, sentinel_join);
}

/// RFC 20260710 C2b — boxed variant of
/// [`crate::ssa_lower_call_console::open_console_sentinel_branch`]
/// for the per-arg inspect walker: the undefined arm prints through
/// `print_any_inline_top` (no-newline inline joiner) instead of a
/// method Str target. Close with `close_console_sentinel_branch`.
fn open_boxed_sentinel_branch(
    ctx: &mut LowerCtx,
    arg: &Operand,
    arg_ty: Type,
) -> Option<crate::ssa::BlockId> {
    if !arg_ty.spells_undef_with_generic_cell() {
        return None;
    }
    let is_undef = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.is_undef_cell, vec![arg.clone()]),
        Type::Bool,
        None,
    );
    let undef_blk = ctx.f.add_block();
    let live_blk = ctx.f.add_block();
    let join_blk = ctx.f.add_block();
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(is_undef),
            then_blk: undef_blk,
            else_blk: live_blk,
        },
    );
    ctx.cur_block = undef_blk;
    let lit = ctx.intern_string_literal("undefined");
    let boxed = ctx.box_to_any(Operand::Value(lit));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.print_any_inline_top, vec![boxed.clone()]),
    );
    ctx.emit_drop_value(boxed, Type::Any);
    ctx.f.set_term(ctx.cur_block, Terminator::Br(join_blk));
    ctx.cur_block = live_blk;
    Some(join_blk)
}
