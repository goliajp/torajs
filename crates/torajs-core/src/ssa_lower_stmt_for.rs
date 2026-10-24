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
use crate::ssa::{BlockId, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::{LocalInfo, LowerCtx};
use crate::ssa_lower_closure_captures::captures_any;
use crate::ssa_lower_push_loop_detect::detect_push_loop_arrays;

/// Chunk 725 — per-iteration state for a `for (let i ...)` binding
/// captured by a closure in the loop body (ES §14.7.4.9
/// CreatePerIterationEnvironment). `outer` is the loop-carried
/// plain alloca the header cond and step operate on; each
/// iteration mints a fresh capture box seeded from it (the body
/// and its closures see that box), and the step block copies the
/// box value back before releasing the iteration's stake. `track`
/// holds the live box pointer (null outside an iteration) so a
/// `break` exit can release the abandoned stake in the after
/// block — the normal exit reaches it as null.
struct PerIterVar {
    name: String,
    outer: LocalInfo,
    track: ValueId,
    boxed: Option<ValueId>,
}

/// Names declared by the for-init that closures in the loop capture.
/// Empty (the shared-box lowering) when a closure the init makes
/// captures one of the head's `let` names: it sees the binding before
/// the first iteration's copy (§14.7.4.4), which the per-iteration
/// rewrite would break; the async gate keeps such a body on the state
/// machine (`desugar_async_ssa_gate`).
fn per_iter_candidates(ctx: &LowerCtx, init: Option<&Stmt>) -> Vec<String> {
    fn decls<'a>(s: &'a Stmt, out: &mut Vec<(bool, &'a String, ExprId)>) {
        match s {
            Stmt::LetDecl {
                mutable,
                is_var,
                name,
                init,
                ..
            } => out.push((*mutable && !*is_var, name, *init)),
            Stmt::Multi(v) => v.iter().for_each(|s| decls(s, out)),
            _ => {}
        }
    }
    let mut head = Vec::new();
    init.into_iter().for_each(|s| decls(s, &mut head));
    let lets: Vec<&str> = head.iter().filter(|d| d.0).map(|d| d.1.as_str()).collect();
    if head.iter().any(|d| captures_any(ctx.ast, d.2, &lets)) {
        return Vec::new();
    }
    let captured = head
        .iter()
        .filter(|d| ctx.escape_captured_lets.contains(d.1));
    captured.map(|d| d.1.clone()).collect()
}

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
    let candidates = per_iter_candidates(ctx, init);
    for name in &candidates {
        ctx.escape_captured_lets.remove(name);
    }
    if let Some(i) = init {
        ctx.lower_stmt(i);
    }
    for name in &candidates {
        ctx.escape_captured_lets.insert(name.clone());
    }
    let mut per_iter = setup_per_iter_vars(ctx, candidates);
    let (reserve_emitted, slow) = emit_push_loop_reserve(ctx, init, cond, step, body);
    lower_loop(ctx, cond, step, body, &mut per_iter);
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
        lower_loop(ctx, cond, step, body, &mut per_iter);
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
    per_iter: &mut [PerIterVar],
) {
    let header = ctx.f.add_block();
    let body_blk = ctx.f.add_block();
    let step_blk = ctx.f.add_block();
    let after = ctx.f.add_block();

    ctx.f.set_term(ctx.cur_block, Terminator::Br(header));

    ctx.cur_block = header;
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
    mint_per_iter_boxes(ctx, per_iter);
    ctx.lower_stmt(body);
    if ctx.cur_open() {
        ctx.f.set_term(ctx.cur_block, Terminator::Br(step_blk));
    }
    ctx.loop_stack.pop();

    ctx.cur_block = step_blk;
    close_per_iter_boxes(ctx, per_iter);
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
    release_broken_iter_boxes(ctx, per_iter);
}

/// Chunk 725 — allocate the per-iteration tracking state for each
/// captured Copy for-init binding: a `track` slot (initialized
/// null) that carries the live box pointer across the loop's edges
/// so the break path can release an abandoned iteration's stake.
///
/// RFC 20260710 C3 (chunk 749) — MUTATED non-Copy bindings ride the
/// same rewrite (the closure-shared box makes same-iteration writes
/// visible per ES §14.7.4.9); never-written captures, Copy or not,
/// keep the env snapshot (indistinguishable from sharing). Any /
/// Substr stay recorded C4 boundaries.
fn setup_per_iter_vars(ctx: &mut LowerCtx, candidates: Vec<String>) -> Vec<PerIterVar> {
    let mut per_iter = Vec::new();
    for name in candidates {
        let Some(outer) = ctx.locals.get(&name).copied() else {
            continue;
        };
        if !(ctx.mutated_captured_lets.contains(&name) && outer.ty != Type::Substr) {
            continue;
        }
        let track = ctx.alloca(Type::Ptr, Some("__periter_box"));
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(Operand::ConstPtrNull, Operand::Value(track), 0),
        );
        per_iter.push(PerIterVar {
            name,
            outer,
            track,
            boxed: None,
        });
    }
    per_iter
}

/// Chunk 725 — mint this iteration's binding at the body block
/// head: a fresh capture box seeded from the outer slot. The body
/// (and every closure it constructs) resolves the name to this box.
fn mint_per_iter_boxes(ctx: &mut LowerCtx, per_iter: &mut [PerIterVar]) {
    for pv in per_iter {
        let cur = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(pv.outer.ty, Operand::Value(pv.outer.slot), 0),
            pv.outer.ty,
            None,
        );
        // RFC 20260710 C3 — a non-Copy seed takes the box's own
        // stake (the outer slot keeps its); the close/break release
        // routes through `capture_box_drop_heap`.
        if !pv.outer.ty.is_copy() {
            ctx.emit_owned_result_inc(Operand::Value(cur), pv.outer.ty);
        }
        let boxed = ctx.emit_capture_boxed(pv.outer.ty, Operand::Value(cur));
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(Operand::Value(boxed), Operand::Value(pv.track), 0),
        );
        pv.boxed = Some(boxed);
        if !pv.outer.ty.is_copy() {
            // Closures constructed in the body capture this box
            // byref (write_captures / assign-ident boxed lanes).
            ctx.boxed_noncopy_lets.insert(pv.name.clone());
        }
        if let Some(ann) = ctx.let_anns.get(&pv.outer.slot).cloned() {
            ctx.let_anns.insert(boxed, ann);
        }
        ctx.locals.insert(
            pv.name.clone(),
            LocalInfo {
                slot: boxed,
                ty: pv.outer.ty,
                moved: true,
                borrowed: false,
                boxed: true,
                scope_depth: pv.outer.scope_depth,
            },
        );
    }
}

/// Chunk 725 — end of the iteration at the step block head
/// (fallthrough AND continue both route here): copy the box value
/// back to the outer slot so the step and next iteration's seed
/// observe body writes, release the iteration's box stake (a
/// closure that captured it holds its own +1), clear the track
/// slot, and re-point the name at the outer slot so the step
/// expression operates on the loop-carried binding (ES: the step
/// runs on the NEXT iteration's environment, so the closed-over
/// box must not see its write).
fn close_per_iter_boxes(ctx: &mut LowerCtx, per_iter: &[PerIterVar]) {
    for pv in per_iter {
        let boxed = pv.boxed.expect("per-iter box minted in body block");
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(pv.outer.ty, Operand::Value(boxed), 0),
            pv.outer.ty,
            None,
        );
        // RFC 20260710 C3 — non-Copy copy-back is a stake handoff:
        // the outer slot incs the (possibly body-rewritten) value
        // BEFORE releasing its old one (self-same when the body
        // never wrote — the pair cancels), and the box release goes
        // through the heap variant (a closure holding the box keeps
        // it and the value alive).
        if !pv.outer.ty.is_copy() {
            ctx.emit_owned_result_inc(Operand::Value(v), pv.outer.ty);
            let old = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Load(pv.outer.ty, Operand::Value(pv.outer.slot), 0),
                pv.outer.ty,
                None,
            );
            ctx.emit_drop_value(Operand::Value(old), pv.outer.ty);
        }
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(Operand::Value(v), Operand::Value(pv.outer.slot), 0),
        );
        let drop_fid = ctx.capture_box_drop_fid(pv.outer.ty);
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Call(drop_fid, vec![Operand::Value(boxed)]),
        );
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(Operand::ConstPtrNull, Operand::Value(pv.track), 0),
        );
        ctx.boxed_noncopy_lets.remove(&pv.name);
        ctx.locals.insert(pv.name.clone(), pv.outer);
    }
}

/// Chunk 725 — a `break` exits mid-iteration with a live box the
/// step block never released; the track slot still holds it (the
/// normal exit path cleared it to null, and the drop null-gates).
/// RFC 20260710 C3 — a non-Copy box releases through the heap
/// variant, and the name re-points at the outer slot so the for-
/// frame walk below drops the outer's own stake (the break path
/// skipped the close's copy-back; on the normal exit these are
/// no-ops — close already restored both).
fn release_broken_iter_boxes(ctx: &mut LowerCtx, per_iter: &[PerIterVar]) {
    for pv in per_iter {
        let t = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(Type::Ptr, Operand::Value(pv.track), 0),
            Type::Ptr,
            None,
        );
        let drop_fid = if pv.outer.ty.is_copy() {
            ctx.intrinsics.capture_box_drop
        } else {
            ctx.intrinsics.capture_box_drop_heap
        };
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Call(drop_fid, vec![Operand::Value(t)]),
        );
        if !pv.outer.ty.is_copy() {
            ctx.boxed_noncopy_lets.remove(&pv.name);
            ctx.locals.insert(pv.name.clone(), pv.outer);
        }
    }
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
