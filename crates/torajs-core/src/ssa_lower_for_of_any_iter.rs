//! `for (x of recv)` where `recv` lowers to `Type::Any` — the
//! unified runtime iteration protocol (RFC 20260704 S5+).
//!
//! One `__torajs_any_iter_next(recv, idx_slot, out_slot)` call per
//! iteration drives every runtime shape behind the erased type:
//! indexed receivers (strings / arrays — the kernel keeps the
//! cursor in `idx_slot` and re-reads the bound live) and the
//! stateful iterator cells the C4+ any-method-call surface mints
//! (`m.keys()` / `m.values()` / `m.entries()`, `arr.values()`).
//! Non-iterable receivers raise a catchable TypeError inside the
//! kernel — the per-step throw check propagates it before the live
//! flag is consulted, so the body never runs.
//!
//! Replaces the S5 indexed two-phase shape (hoisted
//! `__torajs_any_iter_len` + per-iter `recv[i]` runtime index
//! dispatch), which could not carry cursor-bearing iterator cells.
//!
//! Loop shape mirrors `lower_for_of_map_like`: header calls the
//! step kernel and exits on `live == 0`; the body loads the owned
//! AnyValue out-slot and alloca-binds `var_name` as owned
//! (`moved=false` — `close_body_scope`'s drop walk releases the
//! element box each iteration, balancing the kernel's +1).
//!
//! The kernel's class-instance lane (a generator object / any user
//! class with `[Symbol.iterator]()`) parks the iterator it derives in
//! `iter_slot` so the next step resumes the same one. That reference
//! is OURS: the `after` block releases it, which is also the block a
//! `break` lands in. A loop that never took the class lane leaves the
//! slot at `undefined`, whose release is a no-op.

use crate::ast::Stmt;
use crate::ssa::{IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_stmt_for_of::{bind_scoped_local, close_body_scope};

/// The kernel's iterator park slot — an `Any` alloca seeded with
/// `undefined` (the "not yet derived" sentinel). Shared with
/// `ssa_lower_arr_from_any`, the other `any_iter_next` driver.
pub(crate) fn emit_iter_slot(ctx: &mut LowerCtx, name: &str) -> ValueId {
    let slot = ctx.alloca(Type::Any, Some(name));
    let undef = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.any_box,
            vec![Operand::ConstI64(5), Operand::ConstI64(0)],
        ),
        Type::Any,
        None,
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(undef), Operand::Value(slot), 0),
    );
    slot
}

/// Release whatever the kernel parked in the iterator slot. Emitted in
/// the loop's exit block so both a natural finish and a `break` land on
/// it; `undefined` (the never-derived case) drops as a no-op.
pub(crate) fn emit_iter_slot_release(ctx: &mut LowerCtx, iter_slot: ValueId) {
    let iter_val = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::Any, Operand::Value(iter_slot), 0),
        Type::Any,
        None,
    );
    ctx.emit_drop_value(Operand::Value(iter_val), Type::Any);
}

pub(crate) fn lower(
    ctx: &mut LowerCtx,
    src_op: Operand,
    var_name: &str,
    body: &Stmt,
    is_await: bool,
) {
    ctx.scope_stack.push(Vec::new());
    ctx.shadow_stack.push(Vec::new());

    let idx_slot = ctx.alloca(Type::I64, Some("__forof_any_idx"));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::ConstI64(0), Operand::Value(idx_slot), 0),
    );
    let iter_slot = emit_iter_slot(ctx, "__forof_any_iter");
    // RFC 20260901-scope-exit-drops 刀 2 — the parked iterator is an
    // owned local of the iter frame: the frame's close releases it on
    // every way out (the exit block, a throw into an enclosing catch,
    // a labeled jump, a `return`), and the teardown record below only
    // ever owes it the §7.4.9 `return()` call.
    bind_scoped_local(ctx, "__forof_any_iter", iter_slot, Type::Any, false, false);
    let out_slot = ctx.alloca(Type::Any, Some("__forof_any_out"));

    // ES §7.4.9 — the loop owes the iterator a `return()` only when it
    // stops EARLY. Both exits land in `after`, so the natural one
    // records itself here and the close is gated on the flag rather
    // than on which predecessor arrived: a `break` leaves it 0.
    let done_slot = ctx.alloca(Type::I64, Some("__forof_any_done"));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::ConstI64(0), Operand::Value(done_slot), 0),
    );

    let header = ctx.f.add_block();
    let body_blk = ctx.f.add_block();
    let exhausted = ctx.f.add_block();
    let after = ctx.f.add_block();
    ctx.f.set_term(ctx.cur_block, Terminator::Br(header));

    ctx.cur_block = header;
    // `for await` — drain microtasks before each step (same as the
    // typed await route) so the kernel's settle taps see settled
    // cells, then drive the await entry of the same cascade.
    let step_fid = if is_await {
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.microtask_drain, vec![]),
        );
        ctx.intrinsics.any_iter_next_await
    } else {
        ctx.intrinsics.any_iter_next
    };
    let live = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            step_fid,
            vec![
                src_op.clone(),
                Operand::Value(idx_slot),
                Operand::Value(iter_slot),
                Operand::Value(out_slot),
            ],
        ),
        Type::I64,
        None,
    );
    ctx.emit_throw_check(None);
    let done = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Eq, Operand::Value(live), Operand::ConstI64(0)),
        Type::Bool,
        None,
    );
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(done),
            then_blk: exhausted,
            else_blk: body_blk,
        },
    );

    // The iterator said done — it closed itself, so mark the flag and
    // fall into the shared exit.
    ctx.cur_block = exhausted;
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::ConstI64(1), Operand::Value(done_slot), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(after));

    ctx.cur_block = body_blk;
    ctx.scope_stack.push(Vec::new());
    ctx.shadow_stack.push(Vec::new());
    let v_val = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::Any, Operand::Value(out_slot), 0),
        Type::Any,
        None,
    );
    let v_slot = ctx.alloca(Type::Any, Some(var_name));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(v_val), Operand::Value(v_slot), 0),
    );
    bind_scoped_local(ctx, var_name, v_slot, Type::Any, false, false);
    let teardown = crate::ssa_lower_for_of_teardown::ForOfTeardown::Any {
        src: src_op.clone(),
        iter_slot,
    };
    ctx.for_of_teardown_stack.push(teardown.clone());
    // RFC 20260901-scope-exit-drops — body frame already pushed and
    // only closed on fall-through: a jump out owes it (depth = index).
    ctx.loop_stack
        .push(crate::ssa_lower_scope_exit::LoopTargets {
            cont: header,
            brk: after,
            scope_depth: ctx.scope_stack.len() - 1,
            teardown_depth: ctx.for_of_teardown_stack.len(),
        });
    ctx.lower_stmt(body);
    ctx.for_of_teardown_stack.pop();
    let body_open = ctx.cur_open();
    ctx.loop_stack.pop();
    close_body_scope(ctx, header, body_open);

    ctx.cur_block = after;
    let close_blk = ctx.f.add_block();
    let release_blk = ctx.f.add_block();
    let done_flag = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(done_slot), 0),
        Type::I64,
        None,
    );
    let stopped_early = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Eq, Operand::Value(done_flag), Operand::ConstI64(0)),
        Type::Bool,
        None,
    );
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(stopped_early),
            then_blk: close_blk,
            else_blk: release_blk,
        },
    );

    ctx.cur_block = close_blk;
    crate::ssa_lower_for_of_teardown::emit_close(ctx, &teardown);
    ctx.f.set_term(ctx.cur_block, Terminator::Br(release_blk));

    // The iter frame's close releases the parked iterator.
    ctx.cur_block = release_blk;
    ctx.close_scope_frame();
}
