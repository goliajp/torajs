//! `Stmt::For` arm of `LowerCtx::lower_stmt` extracted from
//! [`crate::ssa_lower`] (chunk 145).
//!
//! Pre-extract this arm was 213 LOC inline inside `lower_stmt`
//! (lower_stmt itself is the biggest god-fn left in ssa_lower.rs
//! at ~2900 LOC; arm-by-arm extraction same pattern as the
//! check_stmt_*.rs sibling chain).
//!
//! Lowers `for (init; cond; step) body`. Conceptually:
//! - Push a fresh scope frame for `init`'s let-decl.
//! - **v0.6+1 push-loop pre-reserve fast path** — detect the
//!   canonical `for (let i = 0; i < N; i++) { xs.push(_) }`
//!   pattern via `detect_push_loop_arrays`. When matched: emit
//!   `arr_reserve(xs, len + N)` once before the loop (and hoist
//!   `head_x8 + ARR_DATA_OFF` + len-slot alloca), so the inner
//!   `xs.push(_)` lowerer emits `arr_push_unchecked` — no
//!   per-iter cap-check or grow path. Closes 4 vs-rust losses
//!   (stack-pop / fifo / array-map / generic-id) that all fill an
//!   array in a tight 0..N loop.
//! - Build header / body / step / after blocks. `continue` →
//!   step (so step still runs); `break` → after.
//! - Header evaluates cond (or `true` if absent); branches into
//!   body or after.
//! - After-block syncs the hoisted len_slot back to the array
//!   header so post-loop reads of `arr.length` see the final
//!   count. Restores `PreReserve::unchecked_for` to its pre-loop state.
//! - Drop init-scope locals + restore shadowed bindings.

use crate::ast::{ExprId, Stmt};
use crate::ssa::{BlockId, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_push_loop_detect::detect_push_loop_arrays;

mod periter;

use periter::PerIterVar;

pub(crate) fn lower(
    ctx: &mut LowerCtx,
    init: Option<&Stmt>,
    cond: Option<ExprId>,
    step: Option<ExprId>,
    body: &Stmt,
) {
    ctx.scope_stack.push(Vec::new());
    ctx.shadow_stack.push(Vec::new());
    // Chunk 725 — captured for-init lets lower to a PLAIN alloca
    // here (the loop-carried "outer" slot); each iteration re-boxes
    // it below. Pre-fix they lowered to one shared capture box, so
    // every iteration's closure observed the post-loop value
    // (`m = (x) => x + i` answered i's final 1, bun/ES 0) and the
    // box stake leaked (the for frame walk Copy-skips it).
    // A binding a closure the initializer makes captures keeps its
    // box through the initializer (that closure sees the head's own
    // binding, §14.7.4.4), and the loop then starts from its value.
    let (candidates, in_init) = periter::per_iter_candidates(ctx, init);
    for name in candidates.iter().filter(|n| !in_init.contains(n)) {
        ctx.escape_captured_lets.remove(name);
    }
    if let Some(i) = init {
        ctx.lower_stmt(i);
    }
    for name in &candidates {
        ctx.escape_captured_lets.insert(name.clone());
    }
    periter::detach_init_boxes(ctx, &in_init);
    let per_iter = periter::setup_per_iter_vars(ctx, candidates);
    let (reserve_emitted, slow) = emit_push_loop_reserve(ctx, init, cond, step, body);
    lower_loop(ctx, cond, step, body, &per_iter);
    for name in &reserve_emitted {
        if let Some(state) = ctx.prereserve.unchecked_for.get(name).copied() {
            ctx.emit_prereserved_len_writeback(state);
        }
    }
    for name in &reserve_emitted {
        ctx.prereserve.unchecked_for.remove(name);
    }
    // the trips may outgrow what an array can hold: the loop runs again
    // from its first trip with checked pushes, the arrays' reserve
    // entries gone
    if let Some(slow) = slow {
        let join = ctx.f.add_block();
        ctx.f.set_term(ctx.cur_block, Terminator::Br(join));
        ctx.cur_block = slow;
        lower_loop(ctx, cond, step, body, &per_iter);
        ctx.f.set_term(ctx.cur_block, Terminator::Br(join));
        ctx.cur_block = join;
    }
    let frame = ctx.scope_stack.pop().expect("for-init scope");
    let shadows = ctx.shadow_stack.pop().expect("shadow frame");
    for name in &frame {
        let info = match ctx.locals.get(name) {
            Some(i) => *i,
            None => continue,
        };
        // RFC 20260710 — a promoted mutable non-Copy capture in the
        // for-init releases its box stake at loop close.
        if !info.borrowed && ctx.boxed_noncopy_lets.contains(name) {
            let fid = ctx.capture_box_drop_fid(info.ty);
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Call(fid, vec![Operand::Value(info.slot)]),
            );
            continue;
        }
        if info.moved || info.ty.is_copy() || ctx.stack_alloced_locals.contains(name) {
            continue;
        }
        let val = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(info.ty, Operand::Value(info.slot), 0),
            info.ty,
            None,
        );
        ctx.emit_drop_value(Operand::Value(val), info.ty);
    }
    for name in frame {
        ctx.boxed_noncopy_lets.remove(&name);
        ctx.locals.remove(&name);
    }
    for (name, prev) in shadows {
        if ctx.binding_is_boxed_noncopy(&name, &prev) {
            ctx.boxed_noncopy_lets.insert(name.clone());
        }
        ctx.locals.insert(name, prev);
    }
}

/// The loop proper: header, body and step, ending in the open block
/// the loop leaves by, its broken iteration's boxes released.
fn lower_loop(
    ctx: &mut LowerCtx,
    cond: Option<ExprId>,
    step: Option<ExprId>,
    body: &Stmt,
    per_iter: &[PerIterVar],
) {
    let header = ctx.f.add_block();
    let body_blk = ctx.f.add_block();
    let step_blk = ctx.f.add_block();
    let after = ctx.f.add_block();

    periter::mint_first_boxes(ctx, per_iter);
    ctx.f.set_term(ctx.cur_block, Terminator::Br(header));

    ctx.cur_block = header;
    periter::reload_boxes(ctx, per_iter);
    let c = match cond {
        Some(eid) => {
            let raw = ctx.lower_expr(eid);
            let c = ctx.coerce_to_bool(raw);
            // Chunk 636 — release an owned condition temp after the
            // truthiness test (see ssa_lower_stmt_if.rs).
            ctx.release_owned_temp(eid, &raw);
            c
        }
        None => Operand::ConstBool(true),
    };
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: c,
            then_blk: body_blk,
            else_blk: after,
        },
    );

    // RFC 20260901-scope-exit-drops — the for-init frame is closed by
    // the `after` block on every path, so a jump only owes the body's
    // frames (pushed by `lower_stmt(body)` below): depth = `len()`.
    ctx.loop_stack
        .push(crate::ssa_lower_scope_exit::LoopTargets {
            cont: Some(step_blk),
            brk: after,
            scope_depth: ctx.scope_stack.len(),
            teardown_depth: ctx.for_of_teardown_stack.len(),
        });
    ctx.cur_block = body_blk;
    ctx.lower_stmt(body);
    if ctx.cur_open() {
        ctx.f.set_term(ctx.cur_block, Terminator::Br(step_blk));
    }
    ctx.loop_stack.pop();

    ctx.cur_block = step_blk;
    periter::renew_boxes(ctx, per_iter);
    if let Some(eid) = step {
        // The step's value is read by nobody — the `Stmt::Expr`
        // contract: an Ident-target assignment mints no consumer
        // stake, and an owned temp (a call answering an object or a
        // string) is released here. Neither happened before, so a
        // refcounted step leaked one reference per iteration.
        let saved_discard_expr = ctx.discarded_stmt_expr;
        ctx.discarded_stmt_expr = Some(eid);
        let op = ctx.lower_expr(eid);
        ctx.discarded_stmt_expr = saved_discard_expr;
        ctx.release_owned_temp(eid, &op);
    }
    if ctx.cur_open() {
        ctx.f.set_term(ctx.cur_block, Terminator::Br(header));
    }

    ctx.cur_block = after;
    periter::release_boxes(ctx, per_iter);
}

/// v0.6+1 push-loop pre-reserve fast path (see module doc): detect
/// the canonical `for (let i = 0; i < N; i++) { xs.push(_) }`
/// pattern and reserve capacity once before the loop so the body's
/// pushes skip the per-iter grow check. Answers the array names a
/// matching reserve was emitted for, and the block the length test
/// leaves for when the trips may outgrow an array (see
/// `emit_reserve_slots`).
fn emit_push_loop_reserve(
    ctx: &mut LowerCtx,
    init: Option<&Stmt>,
    cond: Option<ExprId>,
    step: Option<ExprId>,
    body: &Stmt,
) -> (Vec<String>, Option<BlockId>) {
    let pushed_arrays = detect_push_loop_arrays(ctx.ast, init, cond, step, body);
    // Only a body whose every push argument is inert may keep the
    // length word in a register until the loop ends — see
    // `PreReserveState::defer_len`.
    let filled: &[String] = pushed_arrays.as_ref().map_or(&[], |(_, n)| n);
    let defer_len = crate::ssa_lower_push_loop_detect::push_args_all_inert(ctx, body, filled);
    let mut reserve_emitted: Vec<String> = Vec::new();
    let mut slow = None;
    if let Some((bound_eid, names)) = &pushed_arrays {
        // The install gate: the bound is read once here and trusted
        // for the whole loop, and each array is reserved on its own
        // length. See `lower_reserve_bound` for both obligations.
        let Some(bound_op) =
            crate::ssa_lower_push_loop_detect::lower_reserve_bound(ctx, *bound_eid, names, body)
        else {
            return (reserve_emitted, slow);
        };
        for (name, per_trip) in crate::ssa_lower_arr_prereserve::push_counts(names) {
            let Some(info) = ctx.locals.get(&name).copied() else {
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
            let target_cap =
                ctx.emit_reserve_slots(info.ty, cur_arr, bound_op, per_trip, &mut slow);
            let reserved = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(
                    ctx.intrinsics.arr_reserve,
                    vec![Operand::Value(cur_arr), Operand::Value(target_cap)],
                ),
                info.ty,
                None,
            );
            // B1 — reserve never moves the cell.
            ctx.emit_prereserve_entry_gate(reserved, bound_op);
            let state = ctx.emit_prereserved_state(reserved, defer_len);
            ctx.prereserve.unchecked_for.insert(name.clone(), state);
            reserve_emitted.push(name.clone());
        }
    }
    (reserve_emitted, slow)
}
