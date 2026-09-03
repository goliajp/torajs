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
use crate::ssa::{InstKind, Operand, Terminator, Type};
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

/// Borrow judgement for a console.log argument — shared by the
/// single-arg lane and the chunk-808 multi-arg phase-1 stake
/// (RFC 20260711 follow-up: the multi-arg copy of this match was
/// missing the Index / OptChain / This arms, so
/// `console.log(m[0], m[1])` treated container element reads as
/// fresh temps and dropped them without a matching inc — the
/// elements freed under the still-live array, probe `x1|x1`).
///
/// Chunk 570 — container Index reads are borrows (the slot owns the
/// elem; dropping the read stole the slot's stake and the array's
/// death then freed the source — UAF, probe-proven). String indexing
/// stays owned: `s[i]` mints a fresh Substr view (chunk 561 family
/// predicate). OptChain / This align with expr_is_fresh_owned's
/// borrow set. Chunk 717 — reads recorded in `owned_member_reads`
/// (any-member / literal-key Index / OptChain / OptIndex / Closure
/// expando lanes) answer owned, so the predicate runs AFTER the
/// lowering that records them.
pub(crate) fn console_arg_is_borrow(ctx: &LowerCtx<'_>, arg_id: ExprId) -> bool {
    match ctx.ast.get_expr(arg_id) {
        Expr::Ident(_) | Expr::This => true,
        Expr::Member { .. } | Expr::OptChain { .. } | Expr::OptIndex { .. } => {
            !ctx.owned_member_reads.contains(&arg_id)
        }
        Expr::Index { obj, .. } => {
            !matches!(ctx.expr_types.get(obj), Some(crate::check::Type::String))
                && !ctx.owned_member_reads.contains(&arg_id)
        }
        _ => false,
    }
}

/// `console.<m>(v)` — type-specific print target. Substr gets a
/// one-time own copy; primitives and Str pass straight through.
fn lower_single_arg(ctx: &mut LowerCtx<'_>, method: &'static str, arg_id: ExprId) -> Operand {
    let arg = ctx.lower_expr(arg_id);
    let arg_ty = ctx.operand_ty(&arg);
    let is_borrow = console_arg_is_borrow(ctx, arg_id);
    let cur_block = ctx.cur_block;
    // RFC 20260725-fallthrough-return knife 3 — a `void` / `undefined`
    // typed value carries no slot of its own (it lands on the default
    // I64 slot), so the dispatch table below reached its `print_i64`
    // catch-all and printed `0`. The statement-level fast path has a
    // checker-type gate for exactly this (`lower_top_stmt`), which is
    // why only the in-function form diverged: `const v = g();
    // console.log(v)` printed `undefined` at top level and `0` inside
    // any function body. `Void` joins `Undefined` here because
    // `Promise<void>.value` — what `await` desugars to — answers
    // `Void`, and a `void` value at runtime *is* `undefined`. The
    // argument is lowered above so a void call still runs for effect;
    // its operand carries no value.
    if matches!(
        ctx.expr_types.get(&arg_id),
        Some(crate::check::Type::Undefined | crate::check::Type::Void)
    ) {
        let lit = ctx.intern_string_literal("undefined");
        let target = ctx.console_print_target(Type::Str);
        ctx.emit_console_print(method, target, Operand::Value(lit));
        return Operand::ConstI64(0);
    }
    if arg_ty == Type::Substr {
        let substr_to_owned = ctx.intrinsics.substr_to_owned;
        let owned = ctx.f.append_inst(
            cur_block,
            InstKind::Call(substr_to_owned, vec![arg.clone()]),
            Type::Str,
            None,
        );
        let target = ctx.console_print_target(Type::Str);
        ctx.emit_console_print(method, target, Operand::Value(owned));
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
    if arg_ty == Type::F64 && crate::ssa_lower_undef_f64_source::is_undef_f64_source(ctx, arg_id) {
        return lower_print_f64_or_undef(ctx, method, arg);
    }
    let is_str = arg_ty == Type::Str;
    let target = ctx.console_print_target(arg_ty);
    let sentinel_join = open_console_sentinel_branch(ctx, method, &arg, arg_ty);
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
    ctx.emit_console_print(method, target, arg.clone());
    close_console_sentinel_branch(ctx, sentinel_join);
    if !is_borrow {
        if is_str {
            ctx.emit_drop_value(arg, Type::Str);
        } else if arg_ty.is_refcounted() && ctx.expr_owned_shape(arg_id) {
            // Chunk 717 — an owned any-member read printed directly
            // (`console.log(re.source)`): print_any borrows, so the
            // read's stake releases here. Chunk 721 widened the gate
            // to every owned shape (Call / OptCall / New / BinOp
            // results answer fresh boxes per the RFC 20260705
            // invariant): `console.log(mk(i))` leaked its fresh Str
            // box per call (probe c721a 24.2MB vs 6.4MB flat).
            // Ternary / Nullish over owned temps stay on the L3b
            // ledger (they answer borrows of their branch results).
            //
            // Rotation 542 — the gate used to name Type::Any alone,
            // which left every OTHER refcounted temp printed here
            // with no release site at all. 200k churn, AOT product
            // RSS against 1.44MB flat: `console.log([1,2])` 27.2MB,
            // `console.log(new Date(0))` 21.1MB, `console.log({a:1})`
            // 14.4MB, `console.log(2n)` 21.3MB. Str keeps its own
            // arm above (a wider `!is_borrow` gate, unchanged); a
            // possibly-sentinel operand is safe here because the
            // generic undefined cell carries FLAG_STATIC_LITERAL,
            // which short-circuits inc / dec / drop.
            ctx.emit_drop_value(arg, arg_ty);
        }
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
        let s_op = ctx.coerce_to_str(arg.clone(), arg_ty);
        // Chunk 717 — an owned any-member read consumed by the join:
        // the Str coercion mints a fresh cell (borrowing the box), so
        // the read's own stake releases here. Chunk 721 widened the
        // gate to every owned Any shape (probe c721b: a multi-arg
        // `console.log("pfx", mk(i))` leaked the call's fresh box).
        if arg_ty == Type::Any && ctx.expr_owned_shape(aid) {
            ctx.emit_drop_value(arg, Type::Any);
        }
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
    let target = ctx.console_print_target(Type::Str);
    let final_str = acc.unwrap();
    ctx.emit_console_print(method, target, final_str.clone());
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
            Operand::ConstI64(crate::ssa_lower_undef_f64_source::F64_UNDEF_SENTINEL_BITS as i64),
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
    let str_target = ctx.console_print_target(Type::Str);
    ctx.emit_console_print(method, str_target, Operand::Value(sentinel));
    ctx.f.set_term(ctx.cur_block, Terminator::Br(merge));
    ctx.cur_block = num_blk;
    let f64_target = ctx.console_print_target(Type::F64);
    ctx.emit_console_print(method, f64_target, arg);
    ctx.f.set_term(ctx.cur_block, Terminator::Br(merge));
    ctx.cur_block = merge;
    Operand::ConstI64(0)
}

/// RFC 20260710 C2b — open the console sentinel branch for an
/// Obj/Arr/Closure arg: those slots may hold the generic undefined
/// oddball cell (Nullable slot), which the typed struct / arr /
/// closure printers would walk as a live header. Branch on identity:
/// the sentinel prints "undefined" through the method's Str target;
/// the caller emits the live-cell path and closes with
/// [`close_console_sentinel_branch`]. Unconditional probe — the
/// checker's console fast-path does not record arg expr types, so a
/// Nullable-source gate can't see them; console is an io slow path,
/// one probe call is free. `None` for every other operand type.
pub(crate) fn open_console_sentinel_branch(
    ctx: &mut LowerCtx<'_>,
    method: &'static str,
    arg: &Operand,
    arg_ty: Type,
) -> Option<crate::ssa::BlockId> {
    use crate::ssa::IPred;
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
    let str_target = ctx.console_print_target(Type::Str);
    ctx.emit_console_print(method, str_target, Operand::Value(lit));
    ctx.f.set_term(ctx.cur_block, Terminator::Br(join_blk));
    // The same slot spells `null` with the in-band 0 — that is the
    // whole reason a pointer-shaped `T | null` needs no box tax — and
    // the live-cell tail hands whatever it finds to a typed printer.
    // For an Obj / Arr / Closure that printer is `print_anyv`, which
    // read the raw 0 as a NaN box and answered `[unknown-any-tag]`:
    // `let q: {x: number} | null = null; console.log(q)` printed a
    // diagnostic string instead of `null`. The oddball needs the same
    // second test its undefined sibling already has.
    ctx.cur_block = live_blk;
    let is_null = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Eq, arg.clone(), Operand::ConstPtrNull),
        Type::Bool,
        None,
    );
    let null_blk = ctx.f.add_block();
    let cell_blk = ctx.f.add_block();
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(is_null),
            then_blk: null_blk,
            else_blk: cell_blk,
        },
    );
    ctx.cur_block = null_blk;
    let null_lit = ctx.intern_string_literal("null");
    let null_target = ctx.console_print_target(Type::Str);
    ctx.emit_console_print(method, null_target, Operand::Value(null_lit));
    ctx.f.set_term(ctx.cur_block, Terminator::Br(join_blk));
    ctx.cur_block = cell_blk;
    Some(join_blk)
}

/// Close a branch opened by [`open_console_sentinel_branch`] (or the
/// multiarg walker's boxed variant): route the live-cell tail into
/// the join block. No-op when the branch never opened.
pub(crate) fn close_console_sentinel_branch(
    ctx: &mut LowerCtx<'_>,
    join: Option<crate::ssa::BlockId>,
) {
    if let Some(join_blk) = join {
        ctx.f.set_term(ctx.cur_block, Terminator::Br(join_blk));
        ctx.cur_block = join_blk;
    }
}
