//! `LowerCtx::lower_for_of_iter_protocol` extracted from
//! [`crate::ssa_lower::LowerCtx`] (chunk 143).
//!
//! Pre-extract this method was 218 LOC inline on `LowerCtx` (over
//! the 200-line god-fn hard limit per `torajs-file-size-debt`).
//! Body verbatim moved here as a free fn taking `&mut LowerCtx`;
//! the method-side stays as a thin 3-line delegate so the
//! call-site shape in `Stmt::ForOf` lowering is unchanged.
//!
//! Lowers a P5.3 Phase B `for-of` over a user-class iterator
//! protocol receiver. Conceptually:
//!
//! ```text
//! let __it = obj.__sym_Symbol_iterator__()
//! while (true) {
//!   let __step = __it.next()
//!   if (__step.done) break
//!   let v = __step.value
//!   <body>
//! }
//! ```
//!
//! `src_op` is the receiver value (`Type::Obj(sid)`). `iter_fid`
//! is the resolved `__cm_<src_class>____sym_Symbol_iterator__`.
//! The returned iter is `Type::Obj(iter_sid)`; we look up its
//! class via `aliases` to find `__cm_<iter_class>__next`, and
//! the returned step struct provides `.done` / `.value` via
//! direct field loads. v is marked `moved + borrowed` so the
//! end-of-body drop pass doesn't double-dec the step's owned rc.

use crate::ast::{ExprId, Stmt};
use crate::ssa::{FuncId, IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::{LocalInfo, LowerCtx};
use crate::ssa_lower_for_of_iter_protocol_plan::{IterPlan, resolve_iter_plan};
use crate::ssa_lower_stmt_for_of::{bind_scoped_local, close_body_scope};

/// Resolve the iterator class behind `iter_sid`, its `next` method,
/// and the field offsets of the IteratorResult struct `next` returns.
/// One §7.4.6 IteratorNext — build `next()`'s argument list, call it,
/// and let a throw out.
///
/// `next()` is user code (a generator body runs here), and an aborted
/// fn returns the throw sentinel, so reading `.done` off the result
/// before the check is a wild deref: `for (const v of gen)` over a
/// generator that throws on its first step was a SIGSEGV.
#[allow(clippy::too_many_arguments)]
fn emit_next_call(
    ctx: &mut LowerCtx,
    iter_load: crate::ssa::ValueId,
    next_fid: FuncId,
    next_defaults: &[ExprId],
    next_param_tys: &[Type],
    next_call_ty: Type,
    step_ret_ty: Type,
) -> crate::ssa::ValueId {
    let mut next_argv: Vec<Operand> = Vec::with_capacity(1 + next_defaults.len());
    next_argv.push(Operand::Value(iter_load));
    for d in next_defaults {
        let op = ctx.lower_expr(*d);
        next_argv.push(op);
    }
    // Widen / box each defaulted arg into its declared param lane (an
    // `any`-yield generator's numeric-zero placeholder default has to
    // reach an Any param boxed).
    let coerce_owned = if next_param_tys.len() == next_argv.len() {
        crate::ssa_lower_call_terminal::coerce_args_by_param_tys(
            ctx,
            &next_param_tys[1..],
            next_defaults,
            &mut next_argv[1..],
        )
    } else {
        Vec::new()
    };
    let step_val = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(next_fid, next_argv),
        next_call_ty,
        None,
    );
    for (op, ty) in coerce_owned {
        ctx.emit_drop_value(op, ty);
    }
    // ES §7.4.6 IteratorNext — a step that throws forwards the abrupt
    // completion. `next()` is user code (a generator body runs here), and
    // an aborted fn returns the throw sentinel, so reading `.done` off it
    // is a wild deref: `for (const v of gen)` over a generator that throws
    // on its first step was a SIGSEGV.
    ctx.emit_throw_check(None);
    if next_call_ty == step_ret_ty {
        return step_val;
    }
    // §14.7.5.10 — the async form awaits the step before reading it.
    crate::ssa_lower_for_of_iter_protocol_await::emit_await_step(ctx, step_val, step_ret_ty)
}

/// The loop's shared exit — §7.4.9 IteratorClose gated on how the loop
/// stopped, then the iterator's own release.
///
/// Both exits arrive at one block, so the close cannot be decided by
/// predecessor; `done_slot` carries which one it was (0 = stopped
/// early). This lane keeps its iterator in a TYPED local rather than
/// an `Any` park slot, so it closes through the value-shaped face —
/// the box is rc-neutral and the stake stays in the slot, released
/// below either way. An iterator declaring no `return` closes as a
/// no-op.
fn emit_loop_exit(
    ctx: &mut LowerCtx,
    after: crate::ssa::BlockId,
    iter_slot: crate::ssa::ValueId,
    done_slot: crate::ssa::ValueId,
    iter_ret_ty: Type,
) {
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

    let teardown = crate::ssa_lower_for_of_teardown::ForOfTeardown::Typed {
        iter_slot,
        iter_ret_ty,
    };
    ctx.cur_block = close_blk;
    crate::ssa_lower_for_of_teardown::emit_close(ctx, &teardown);
    ctx.f.set_term(ctx.cur_block, Terminator::Br(release_blk));

    // The caller closes the iter frame here, which releases the slot.
    ctx.cur_block = release_blk;
}

pub(crate) fn lower_for_of_iter_protocol(
    ctx: &mut LowerCtx,
    src_op: Operand,
    iter_fid: FuncId,
    var_name: &str,
    body: &Stmt,
    src_class: &str,
    is_await: bool,
) {
    let iter_ret_ty = ctx.f_ret_type_hint(iter_fid);
    let Type::Obj(iter_sid) = iter_ret_ty else {
        panic!(
            "ssa-lower: for-of protocol on class `{src_class}` — `[Symbol.iterator]()` must return a class instance, got {iter_ret_ty:?}"
        );
    };
    let iter_val = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(iter_fid, vec![src_op]),
        iter_ret_ty,
        None,
    );
    // `[Symbol.iterator]()` is user code and can throw; what an aborted
    // fn answers is a sentinel, not an iterator (see the `next()` check
    // below).
    ctx.emit_throw_check(None);

    let plan = resolve_iter_plan(ctx, iter_sid, src_class, is_await);
    let IterPlan {
        next_fid,
        next_defaults,
        next_param_tys,
        next_call_ty,
        step_ret_ty,
        value_ty,
        value_off,
        done_off,
    } = plan;

    ctx.scope_stack.push(Vec::new());
    ctx.shadow_stack.push(Vec::new());
    let iter_slot = ctx.alloca(iter_ret_ty, Some("__forof_it"));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(iter_val), Operand::Value(iter_slot), 0),
    );
    // RFC 20260901-scope-exit-drops 刀 2 — the iterator is an owned
    // local of the iter frame: the frame's close releases it on every
    // way out (the exit block, a throw into an enclosing catch, a
    // labeled jump, a `return`); the teardown record only owes it the
    // §7.4.9 `return()` call.
    bind_scoped_local(ctx, "__forof_it", iter_slot, iter_ret_ty, false, false);

    // ES §7.4.9 — a `return()` is owed only on an EARLY stop. Both
    // exits share `after`, so the natural one records itself and the
    // close is gated on the flag; a `break` leaves it 0. Mirror of the
    // `any` lane's shape (`ssa_lower_for_of_any_iter`).
    let done_slot = ctx.alloca(Type::I64, Some("__forof_proto_done"));
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
    let iter_load = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(iter_ret_ty, Operand::Value(iter_slot), 0),
        iter_ret_ty,
        None,
    );
    let step_val = emit_next_call(
        ctx,
        iter_load,
        next_fid,
        &next_defaults,
        &next_param_tys,
        next_call_ty,
        step_ret_ty,
    );
    let done_val = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::Bool, Operand::Value(step_val), done_off),
        Type::Bool,
        None,
    );
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(done_val),
            then_blk: exhausted,
            else_blk: body_blk,
        },
    );

    // The iterator reported done — it closed itself. The final step
    // struct is ours too; it never reaches the body frame that owns
    // the others.
    ctx.cur_block = exhausted;
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::ConstI64(1), Operand::Value(done_slot), 0),
    );
    ctx.emit_drop_value(Operand::Value(step_val), step_ret_ty);
    ctx.f.set_term(ctx.cur_block, Terminator::Br(after));

    ctx.cur_block = body_blk;
    ctx.scope_stack.push(Vec::new());
    ctx.shadow_stack.push(Vec::new());
    // 刀 2 — the step struct is an owned local of the body frame, so a
    // `continue` / throw / labeled jump out of the body releases it
    // with the frame; pre-RFC only the fall-through dropped it.
    let step_slot = ctx.alloca(step_ret_ty, Some("__forof_step"));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(step_val), Operand::Value(step_slot), 0),
    );
    bind_scoped_local(ctx, "__forof_step", step_slot, step_ret_ty, false, false);
    let v_val = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(value_ty, Operand::Value(step_val), value_off),
        value_ty,
        None,
    );
    let v_slot = ctx.alloca(value_ty, Some(var_name));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(v_val), Operand::Value(v_slot), 0),
    );
    {
        let cur_depth = ctx.scope_stack.len() - 1;
        if let Some(prev) = ctx.locals.get(var_name).copied()
            && prev.scope_depth < cur_depth
        {
            ctx.shadow_stack
                .last_mut()
                .expect("shadow frame")
                .push((var_name.to_string(), prev));
        }
        ctx.locals.insert(
            var_name.to_string(),
            LocalInfo {
                slot: v_slot,
                ty: value_ty,
                moved: true,
                borrowed: true,
                scope_depth: cur_depth,
            },
        );
        ctx.scope_stack
            .last_mut()
            .expect("scope frame")
            .push(var_name.to_string());
    }
    ctx.for_of_teardown_stack
        .push(crate::ssa_lower_for_of_teardown::ForOfTeardown::Typed {
            iter_slot,
            iter_ret_ty,
        });
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
    // The body frame's close releases this iteration's step struct
    // along with the body's own locals (the loop variable borrows).
    close_body_scope(ctx, header, body_open);

    emit_loop_exit(ctx, after, iter_slot, done_slot, iter_ret_ty);
    // The iter frame's close releases the iterator.
    ctx.close_scope_frame();
}
