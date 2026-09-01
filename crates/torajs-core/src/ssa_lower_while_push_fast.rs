//! 12-c-1 — while-shape push-loop pre-reserve fast-push.
//!
//! Counterpart to the canonical-`for` install in `Stmt::For`'s arm of
//! `ssa_lower::LowerCtx::lower_stmt`. Recognises the textbook
//!
//! ```ignore
//!   let i: number = 0;
//!   while (i < N) { xs.push(_); ...; i = i + 1; }
//! ```
//!
//! shape and emits the same pre-reserve + inline-push lowering the
//! `for` path does — `arr_reserve(xs, len + N)` once before the loop,
//! per-iter `StoreDyn` to the hoisted `head_x8 + ARR_DATA_OFF + len*8`
//! slot, len_slot bumped, and len written back to the array header
//! after the loop exits. The runtime call to `__torajs_arr_push` is
//! elided entirely on the canonical shape.
//!
//! Caller-side contract: the wiring in `ssa_lower` tracks the
//! immediately-preceding stmt in `Stmt::Block` / `Stmt::Multi` / the
//! `lower_fn` body / `synthesize_main` top-level iteration. When a
//! `Stmt::While` lands after a `let counter = 0` (per
//! [`let_counter_zero_name`](crate::ssa_lower_push_loop_detect::let_counter_zero_name)),
//! caller invokes [`lower_while_inner`] with `Some(counter)` and this
//! module attempts the install; otherwise `None` falls through to the
//! plain while-lower.
//!
//! Lives in its own file to keep `ssa_lower.rs` on its known-debt
//! shrink-only trajectory (`.claude/rules/common/file-size.md`). Same
//! placement rationale as `ssa_lower_push_loop_detect`, `ssa_lower_
//! deque_escape`, `ssa_lower_obj_escape`, `ssa_lower_closure_captures`.

use crate::ast::{ExprId, Stmt};
use crate::ssa::{BinOp as SsaBinOp, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx};
use crate::ssa_lower_push_loop_detect::detect_push_loop_arrays_while;

/// 12-c-1 — `while` lowering, with an optional pre-reserve fast-push
/// install. When `counter_name_opt` is `Some(counter)`, the caller has
/// proven that `counter` is statically 0 at while entry (immediately-
/// preceding `let counter: number = 0` stmt in the surrounding
/// Block/Multi); this function runs [`detect_push_loop_arrays_while`]
/// on `(cond, body)`, and if the shape matches, emits an
/// `arr_reserve(xs, len + bound)` once before the loop, hoists a
/// per-array `len_slot`, and installs each detected `xs` into
/// `ctx.prereserve.unchecked_for` so the inner `xs.push(_)` lower-site
/// emits the inline fast-push (StoreDyn + len bump, no realloc / no
/// per-iter cap-check) — same as the canonical-for path.
/// `counter_name_opt = None` falls through to the plain while.
///
/// Mirrors the for-loop install logic at the top of the `Stmt::For`
/// arm in `ssa_lower::LowerCtx::lower_stmt`; the only structural
/// difference is that while has no `step` block — the step
/// `counter = counter + 1` is the body's last stmt and lowers
/// normally as part of `lower_stmt(body)`.
pub(crate) fn lower_while_inner(
    ctx: &mut LowerCtx<'_>,
    cond: ExprId,
    body: &Stmt,
    counter_name_opt: Option<&str>,
) {
    /* Pre-reserve install — only attempt when the caller passed a
     * counter name (= prev stmt was `let counter = 0`). The detector
     * either returns matched (bound_eid, names) or None; on None we
     * fall through to the plain while-lower below. */
    let mut reserve_emitted: Vec<String> = Vec::new();
    if let Some(counter_name) = counter_name_opt
        && let Some((bound_eid, names)) =
            detect_push_loop_arrays_while(ctx.ast, counter_name, cond, body)
        // The install gate: the bound is read once here and trusted
        // for the whole loop, and each array is reserved on its own
        // length. See `lower_reserve_bound` for both obligations.
        && let Some(bound_op) =
            crate::ssa_lower_push_loop_detect::lower_reserve_bound(ctx, bound_eid, &names)
    {
        // Only a body whose every push argument is inert may keep the
        // length word in a register until the loop ends — see
        // `PreReserveState::defer_len`. The trailing counter step is
        // inert by the same reading.
        let defer_len = crate::ssa_lower_push_loop_detect::push_args_all_inert(ctx, body);
        for name in &names {
            let Some(info) = ctx.locals.get(name).copied() else {
                continue;
            };
            if !matches!(info.ty, Type::Arr(_)) {
                continue;
            }
            // The push site this fast path exists for serves every
            // element type EXCEPT `any`: `Array<Any>.push` needs a
            // (tag, value) pair and returns before the inline-store
            // path is ever consulted (`ssa_lower_call_arr_push`'s
            // P0.10 arm). Claiming such a binding registered the
            // reserve and the cached length, got none of the inline
            // stores, and then wrote the PRE-LOOP length back at the
            // loop exit — erasing every push the runtime helper had
            // made. `let a: any[] = []; for (…) a.push(i);` answered
            // length 0, silently, and any other statement in the body
            // hid it by keeping the binding live.
            if matches!(info.ty, Type::Arr(id) if ctx.arr_layouts[id.0 as usize] == Type::Any) {
                continue;
            }
            let cur_arr = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Load(info.ty, Operand::Value(info.slot), 0),
                info.ty,
                None,
            );
            /* Need cap >= len + bound. Read len, add bound, pass to
             * arr_reserve. */
            let initial_len_v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Load(Type::I64, Operand::Value(cur_arr), ARR_LEN_OFF),
                Type::I64,
                None,
            );
            let target_cap = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::BinOp(
                    SsaBinOp::Add,
                    Operand::Value(initial_len_v),
                    bound_op.clone(),
                ),
                Type::I64,
                None,
            );
            let reserved = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(
                    ctx.intrinsics.arr_reserve,
                    vec![Operand::Value(cur_arr), Operand::Value(target_cap)],
                ),
                info.ty,
                None,
            );
            // B1 — reserve never moves the cell; write-back retired.
            let state = ctx.emit_prereserved_state(reserved, defer_len);
            ctx.prereserve.unchecked_for.insert(name.clone(), state);
            reserve_emitted.push(name.clone());
        }
    }

    let header = ctx.f.add_block();
    let body_blk = ctx.f.add_block();
    let after = ctx.f.add_block();

    ctx.f.set_term(ctx.cur_block, Terminator::Br(header));

    ctx.cur_block = header;
    let raw = ctx.lower_expr(cond);
    let c = ctx.coerce_to_bool(raw.clone());
    // Chunk 636 — release an owned condition temp after the
    // truthiness test (see ssa_lower_stmt_if.rs).
    ctx.release_owned_temp(cond, &raw);
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: c,
            then_blk: body_blk,
            else_blk: after,
        },
    );

    /* M1.7 — `break` jumps to `after`, `continue` jumps to `header`
     * (which re-evaluates cond). Push the loop ctx before lowering
     * body so nested break/continue resolve correctly. */
    // RFC 20260901-scope-exit-drops — no loop-owned frame; a jump
    // owes the body's frames pushed by `lower_stmt(body)`.
    ctx.loop_stack
        .push(crate::ssa_lower_scope_exit::LoopTargets {
            cont: header,
            brk: after,
            scope_depth: ctx.scope_stack.len(),
            teardown_depth: ctx.for_of_teardown_stack.len(),
        });
    ctx.cur_block = body_blk;
    ctx.lower_stmt(body);
    if ctx.cur_open() {
        ctx.f.set_term(ctx.cur_block, Terminator::Br(header));
    }
    ctx.loop_stack.pop();

    ctx.cur_block = after;

    /* Settle the length word for a lane that deferred it, then remove
     * the PreReserve entry so a follow-up while/for on the
     * same array doesn't accidentally inherit it. */
    for name in &reserve_emitted {
        if let Some(state) = ctx.prereserve.unchecked_for.get(name).copied() {
            ctx.emit_prereserved_len_writeback(state);
        }
    }
    for name in &reserve_emitted {
        ctx.prereserve.unchecked_for.remove(name);
    }
}
