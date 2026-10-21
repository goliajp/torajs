//! `Stmt::ForOfSplitIter` arm of `LowerCtx::lower_stmt` extracted
//! from [`crate::ssa_lower`] (chunk 150).
//!
//! P-iter — lowers
//!
//! ```text
//! for (let v of <parent>.split(<sep_lit>)) body
//! ```
//!
//! to a SplitIter loop that yields per-iteration STATIC Substr
//! borrows. Baseline layout:
//!
//! ```text
//! parent_op = lower parent           (Type::Str)
//! sep_op    = lower sep              (Type::Str, STATIC literal)
//! iter_slot = alloca_bytes 48        (SplitIter struct)
//! sub_slot  = alloca_bytes 32        (Substr borrow)
//! v_slot    = alloca Substr ptr → sub_slot (constant for all iters)
//! call __torajs_split_iter_init(iter_slot, parent_op, sep_op)
//!
//! br header
//! header:
//!   ok = call __torajs_split_iter_next(iter_slot, sub_slot)
//!   cond_br ok, body_blk, after
//! body_blk:
//!   <body — reads of `v` load v_slot → always the same sub_slot
//!     ptr; sub_slot's contents refilled by next() each iter>
//!   br header
//! after:
//! ```
//!
//! The iter borrows `parent`; keeping it alive is the loop's job, not
//! the kernel's. A source binding that provably outlives the loop (an
//! owned local or a param the body never writes, not captured by
//! reference) is borrowed with no rc traffic at all. Anything else —
//! a temporary, a binding the body may overwrite — is held by a
//! hidden owned local of an outer frame: a temporary hands its +1
//! over, a borrow is retained first. Being a frame local, it is
//! released on every way out (the exit block, `return`, a throw into
//! an enclosing catch, a labeled jump), which a teardown call at
//! `after:` alone cannot reach. Each yielded substr carries the
//! STATIC_LITERAL flag (set by the helper / the inline scan below) so
//! rc_inc / rc_dec / substr_drop on `v` no-op — exactly matching the
//! borrow semantics, no per-iter teardown IR.
//!
//! ## S7-r2 — inline Latin-1 fast path (loop versioning)
//!
//! The rpn ablation priced the per-token cross-archive
//! `__torajs_split_iter_next` call at 45% of the whole probe (1.40s
//! → 0.77s with the split deleted; the call target can't inline —
//! S7 立项事实 4-①). When the separator is a compile-time 1-byte
//! Latin-1 literal (the dominant `" "` shape — parser/sfi_pass only
//! mint ForOfSplitIter for literal seps), the loop tests a HOISTED
//! `parent is Latin-1` bool once at entry; the slow arm starts with the
//! fast arm's done flag set, and the header dispatches on that flag:
//!
//! - fast arm: inline scan — the parent read eight bytes at a time
//!   the way `memchr` does, each part cut from the word's separator
//!   marks (`ssa_lower_stmt_for_of_split_iter_fast::word`) — + direct
//!   Substr write into `sub_slot`, loop state (`pos` / `done` / the
//!   marks) in mem2reg-liftable scalar slots.
//!   The iter struct is never initialized on this arm: the parent
//!   length comes straight from the Str header.
//! - slow arm: the pre-existing `split_iter_next` call (UTF-16 /
//!   mixed-encoding parents).
//!
//! The dispatch costs the fast arm one perfectly-predicted register
//! test per iteration, the same one that finds it exhausted (a done
//! loop then tells the two arms apart by the hoisted bool); the two
//! arms converge on the SHARED `body_blk` so the body lowers once and
//! `break`/`continue` semantics are unchanged (`continue` → header
//! re-dispatches on the done flag).
//!
//! Fast-arm semantics mirror the kernel's `sep_len == 1` Latin-1
//! branch exactly, including the empty-parent case (one empty chunk
//! is yielded, then exhausted) and the trailing-separator case (an
//! empty tail chunk is yielded when the last byte is the separator).

use crate::ast::{Expr, ExprId, Stmt};
use crate::ssa::{BinOp as SsaBinOp, IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_stmt_for_of::bind_scoped_local;

const PARENT_HOLD: &str = "__split_parent";

pub(crate) fn lower(ctx: &mut LowerCtx, var_name: &str, parent: ExprId, sep: ExprId, body: &Stmt) {
    let parent_op = ctx.lower_expr(parent);
    let sep_op = ctx.lower_expr(sep);
    // The iterator kernel reads its parent as a plain string, so a
    // view parent (`for (const c of row.split(","))` with `row` itself
    // a split element) is copied out first; the loop holds the copy.
    let (parent_op, minted) = if ctx.operand_ty(&parent_op) == Type::Substr {
        let owned = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.substr_to_owned, vec![parent_op]),
            Type::Str,
            None,
        );
        if ctx.expr_transfers_ownership(parent) {
            ctx.emit_drop_value(parent_op, Type::Substr);
        }
        (Operand::Value(owned), true)
    } else {
        (parent_op, false)
    };
    let parent_ty = ctx.operand_ty(&parent_op);

    // Outer frame: owns the parent stake, when the loop needs one.
    ctx.scope_stack.push(Vec::new());
    ctx.shadow_stack.push(Vec::new());
    if parent_ty.is_refcounted() && (minted || !source_outlives_loop(ctx, parent, body)) {
        if !minted && !ctx.expr_transfers_ownership(parent) {
            ctx.emit_owned_result_inc(parent_op, parent_ty);
        }
        let hold = ctx.alloca(parent_ty, Some(PARENT_HOLD));
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(parent_op, Operand::Value(hold), 0),
        );
        bind_scoped_local(ctx, PARENT_HOLD, hold, parent_ty, false, false, false);
    }

    let iter_slot = ctx
        .f
        .append_inst(ctx.cur_block, InstKind::AllocaBytes(48), Type::Ptr, None);
    let sub_slot = ctx
        .f
        .append_inst(ctx.cur_block, InstKind::AllocaBytes(32), Type::Ptr, None);

    // Inner frame: the loop variable, a borrow of `sub_slot` whose
    // STATIC header makes its drop a no-op.
    ctx.scope_stack.push(Vec::new());
    ctx.shadow_stack.push(Vec::new());
    let v_slot = ctx.alloca(Type::Substr, Some(var_name));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(sub_slot), Operand::Value(v_slot), 0),
    );
    bind_scoped_local(ctx, var_name, v_slot, Type::Substr, false, false, false);

    // Compile-time gate for the inline fast arm: 1-char Latin-1
    // literal separator AND a plain Str parent (a Substr parent's
    // payload is NOT at +16 — its slots 16/24 are parent-ptr /
    // offset, so the inline scan must never fire on it).
    let fast_sep_byte: Option<u8> = match ctx.ast.get_expr(sep) {
        Expr::String(s) if s.code_points().count() == 1 => {
            let c = s.code_points().next().expect("1-char literal");
            u8::try_from(c).ok()
        }
        _ => None,
    };
    let fast_sep_byte = fast_sep_byte.filter(|_| parent_ty == Type::Str);

    let header = ctx.f.add_block();
    let body_blk = ctx.f.add_block();
    let after = ctx.f.add_block();

    let mut view = None;
    if let Some(target) = fast_sep_byte {
        let fill_sub = !crate::ast::sfi_safe::split_var_view_reads_only(ctx.ast, body, var_name);
        view = Some(
            crate::ssa_lower_stmt_for_of_split_iter_fast::emit_versioned_header(
                ctx,
                &crate::ssa_lower_stmt_for_of_split_iter_fast::SplitLoop {
                    parent_op,
                    sep_op,
                    iter_slot,
                    sub_slot,
                    v_slot,
                    fill_sub,
                    target,
                    header,
                    body_blk,
                    after,
                },
            ),
        );
    } else {
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.split_iter_init,
                vec![Operand::Value(iter_slot), parent_op, sep_op],
            ),
        );
        ctx.f.set_term(ctx.cur_block, Terminator::Br(header));
        ctx.cur_block = header;
        let ok = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.split_iter_next,
                vec![Operand::Value(iter_slot), Operand::Value(sub_slot)],
            ),
            Type::Bool,
            None,
        );
        ctx.f.set_term(
            ctx.cur_block,
            Terminator::CondBr {
                cond: Operand::Value(ok),
                then_blk: body_blk,
                else_blk: after,
            },
        );
    }

    // RFC 20260901-scope-exit-drops — `break` lands in `after`, which
    // closes both loop frames itself, so a jump owes only the body's
    // own frames: depth = `len()`.
    ctx.loop_stack
        .push(crate::ssa_lower_scope_exit::LoopTargets {
            cont: Some(header),
            brk: after,
            scope_depth: ctx.scope_stack.len(),
            teardown_depth: ctx.for_of_teardown_stack.len(),
        });
    ctx.cur_block = body_blk;
    let has_view = view.is_some();
    ctx.split_views.extend(view);
    ctx.lower_stmt(body);
    if has_view {
        ctx.split_views.pop();
    }
    if ctx.cur_open() {
        ctx.f.set_term(ctx.cur_block, Terminator::Br(header));
    }
    ctx.loop_stack.pop();

    ctx.cur_block = after;
    // `v` borrows the STATIC `sub_slot`; its drop would be a no-op call.
    if let Some(info) = ctx.locals.get_mut(var_name) {
        info.moved = true;
    }
    ctx.close_scope_frame();
    ctx.close_scope_frame();
}

/// True when the parent is a binding whose own stake provably spans
/// the loop, so the iter can borrow it with no rc traffic: a local of
/// this fn that the body never writes and no closure holds by
/// reference, owning its value (`let` / `const`) or borrowing the
/// caller's for the whole call (a param). Borrowed aliases below the
/// fn's top frame (a for-of element) are excluded — their owner can
/// drop the value mid-loop.
fn source_outlives_loop(ctx: &LowerCtx, parent: ExprId, body: &Stmt) -> bool {
    let Expr::Ident(name) = ctx.ast.get_expr(parent) else {
        return false;
    };
    let Some(info) = ctx.locals.get(name) else {
        return false;
    };
    let stake = (!info.moved && !info.borrowed) || (info.borrowed && info.scope_depth == 0);
    stake
        && !info.boxed
        && !ctx.boxed_noncopy_lets.contains(name)
        && !crate::ssa_lower_for_of_written::body_writes_var(ctx, body, name)
}

/// A versioned split loop's part as registers: the per-part length and
/// offset slots both arms fill, and the loop-invariant payload base and
/// encoding test. Reads of the loop variable that only need these
/// (`charCodeAt`, `length`) skip the Substr in `sub_slot`, which the
/// fast arm leaves unwritten when those are the body's only reads.
#[derive(Clone, Copy)]
pub(crate) struct SplitView {
    pub(crate) v_slot: ValueId,
    pub(crate) len_slot: ValueId,
    pub(crate) off_slot: ValueId,
    pub(crate) pbase: ValueId,
    pub(crate) is_lat: ValueId,
}

/// The view whose loop variable `recv` names, if any: an identifier
/// bound to that loop's own slot (not a shadowing inner binding).
pub(crate) fn view_of(ctx: &LowerCtx, recv: ExprId) -> Option<SplitView> {
    let Expr::Ident(name) = ctx.ast.get_expr(recv) else {
        return None;
    };
    let slot = ctx.locals.get(name)?.slot;
    ctx.split_views
        .iter()
        .rev()
        .find(|v| v.v_slot == slot)
        .copied()
}

fn load_slot(ctx: &mut LowerCtx, slot: ValueId) -> ValueId {
    ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(slot), 0),
        Type::I64,
        None,
    )
}

/// `part.length` on a split loop's variable.
pub(crate) fn try_view_length(ctx: &mut LowerCtx, recv: ExprId) -> Option<Operand> {
    let view = view_of(ctx, recv)?;
    Some(Operand::Value(load_slot(ctx, view.len_slot)))
}

/// `part.charCodeAt(pos?)` on a split loop's variable (ES §22.1.3.2):
/// one unsigned bound test against the part length, then a byte read
/// straight from the parent payload when it is Latin-1, else the
/// Substr kernel on the loop variable — only the slow arm reaches
/// that, and it always fills `sub_slot`.
pub(crate) fn try_view_char_code_at(
    ctx: &mut LowerCtx,
    recv: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let view = view_of(ctx, recv)?;
    if args.len() > 1 {
        return None;
    }
    let idx = if args.is_empty()
        || matches!(
            ctx.expr_types.get(&args[0]),
            Some(crate::check::Type::Undefined)
        ) {
        Operand::ConstI64(0)
    } else {
        let n = ctx.lower_to_number_operand(args[0]);
        ctx.coerce_to_i64(n)
    };
    let len = load_slot(ctx, view.len_slot);
    let cur = ctx.cur_block;
    let result_slot = ctx
        .f
        .append_inst(cur, InstKind::Alloca(Type::F64), Type::Ptr, None);
    let in_bounds = ctx.f.append_inst(
        cur,
        InstKind::ICmp(IPred::Ult, idx, Operand::Value(len)),
        Type::Bool,
        None,
    );
    let inb_blk = ctx.f.add_block();
    let oob_blk = ctx.f.add_block();
    let lat_blk = ctx.f.add_block();
    let wide_blk = ctx.f.add_block();
    let join_blk = ctx.f.add_block();
    ctx.f.set_term(
        cur,
        Terminator::CondBr {
            cond: Operand::Value(in_bounds),
            then_blk: inb_blk,
            else_blk: oob_blk,
        },
    );
    ctx.f.append_void(
        oob_blk,
        InstKind::Store(Operand::ConstF64(f64::NAN), Operand::Value(result_slot), 0),
    );
    ctx.f.set_term(oob_blk, Terminator::Br(join_blk));
    ctx.f.set_term(
        inb_blk,
        Terminator::CondBr {
            cond: Operand::Value(view.is_lat),
            then_blk: lat_blk,
            else_blk: wide_blk,
        },
    );

    ctx.cur_block = lat_blk;
    let off = load_slot(ctx, view.off_slot);
    let j = ctx.f.append_inst(
        lat_blk,
        InstKind::BinOp(SsaBinOp::Add, Operand::Value(off), idx),
        Type::I64,
        None,
    );
    let b = ctx.f.append_inst(
        lat_blk,
        InstKind::LoadU8Dyn(Operand::Value(view.pbase), Operand::Value(j)),
        Type::I64,
        None,
    );
    let f = ctx.f.append_inst(
        lat_blk,
        InstKind::SiToFp(Operand::Value(b)),
        Type::F64,
        None,
    );
    ctx.f.append_void(
        lat_blk,
        InstKind::Store(Operand::Value(f), Operand::Value(result_slot), 0),
    );
    ctx.f.set_term(lat_blk, Terminator::Br(join_blk));

    let part = ctx.f.append_inst(
        wide_blk,
        InstKind::Load(Type::Substr, Operand::Value(view.v_slot), 0),
        Type::Substr,
        None,
    );
    let v = ctx.f.append_inst(
        wide_blk,
        InstKind::Call(
            ctx.intrinsics.substr_char_code_at,
            vec![Operand::Value(part), idx],
        ),
        Type::F64,
        None,
    );
    ctx.f.append_void(
        wide_blk,
        InstKind::Store(Operand::Value(v), Operand::Value(result_slot), 0),
    );
    ctx.f.set_term(wide_blk, Terminator::Br(join_blk));

    ctx.cur_block = join_blk;
    let out = ctx.f.append_inst(
        join_blk,
        InstKind::Load(Type::F64, Operand::Value(result_slot), 0),
        Type::F64,
        None,
    );
    Some(Operand::Value(out))
}
