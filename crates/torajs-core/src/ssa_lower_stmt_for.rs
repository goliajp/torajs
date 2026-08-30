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
use crate::ssa::{BinOp as SsaBinOp, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::{ARR_LEN_OFF, LocalInfo, LowerCtx};
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

/// Names declared by the for-init that closures in the loop
/// capture. Answers empty (= keep the legacy shared-box lowering)
/// when any init expression contains a closure itself — an init
/// closure captures the INITIAL binding per spec, which the
/// per-iteration rewrite would break (recorded edge).
fn per_iter_candidates(ctx: &LowerCtx, init: Option<&Stmt>) -> Vec<String> {
    fn collect(ctx: &LowerCtx, s: &Stmt, out: &mut Vec<String>, bail: &mut bool) {
        match s {
            Stmt::LetDecl { name, init, .. } => {
                if crate::ssa_lower_closure_captures::expr_contains_closure(ctx.ast, *init) {
                    *bail = true;
                    return;
                }
                if ctx.escape_captured_lets.contains(name) {
                    out.push(name.clone());
                }
            }
            Stmt::Multi(v) => {
                for s in v {
                    collect(ctx, s, out, bail);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    let mut bail = false;
    if let Some(s) = init {
        collect(ctx, s, &mut out, &mut bail);
    }
    if bail { Vec::new() } else { out }
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
    let reserve_emitted = emit_push_loop_reserve(ctx, init, cond, step, body);
    let header = ctx.f.add_block();
    let body_blk = ctx.f.add_block();
    let step_blk = ctx.f.add_block();
    let after = ctx.f.add_block();

    ctx.f.set_term(ctx.cur_block, Terminator::Br(header));

    ctx.cur_block = header;
    let c = match cond {
        Some(eid) => {
            let raw = ctx.lower_expr(eid);
            let c = ctx.coerce_to_bool(raw.clone());
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

    ctx.loop_stack.push((step_blk, after));
    ctx.cur_block = body_blk;
    mint_per_iter_boxes(ctx, &mut per_iter);
    ctx.lower_stmt(body);
    if ctx.cur_open() {
        ctx.f.set_term(ctx.cur_block, Terminator::Br(step_blk));
    }
    ctx.loop_stack.pop();

    ctx.cur_block = step_blk;
    close_per_iter_boxes(ctx, &per_iter);
    if let Some(eid) = step {
        let _ = ctx.lower_expr(eid);
    }
    if ctx.cur_open() {
        ctx.f.set_term(ctx.cur_block, Terminator::Br(header));
    }

    ctx.cur_block = after;
    release_broken_iter_boxes(ctx, &per_iter);
    for name in &reserve_emitted {
        if let Some(state) = ctx.prereserve.unchecked_for.get(name).copied() {
            ctx.emit_prereserved_len_writeback(state);
        }
    }
    for name in &reserve_emitted {
        ctx.prereserve.unchecked_for.remove(name);
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

/// Chunk 725 — allocate the per-iteration tracking state for each
/// captured Copy for-init binding: a `track` slot (initialized
/// null) that carries the live box pointer across the loop's edges
/// so the break path can release an abandoned iteration's stake.
///
/// RFC 20260710 C3 (chunk 749) — MUTATED non-Copy bindings ride the
/// same rewrite (the closure-shared box makes same-iteration writes
/// visible per ES §14.7.4.9); never-written non-Copy captures keep
/// the env-owns snapshot (indistinguishable from sharing). Any /
/// Substr stay recorded C4 boundaries.
fn setup_per_iter_vars(ctx: &mut LowerCtx, candidates: Vec<String>) -> Vec<PerIterVar> {
    let mut per_iter = Vec::new();
    for name in candidates {
        let Some(outer) = ctx.locals.get(&name).copied() else {
            continue;
        };
        if !outer.ty.is_copy()
            && !(ctx.mutated_captured_lets.contains(&name) && outer.ty != Type::Substr)
        {
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
        ctx.locals.insert(
            pv.name.clone(),
            LocalInfo {
                slot: boxed,
                ty: pv.outer.ty,
                moved: true,
                borrowed: false,
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
/// matching reserve was emitted for.
fn emit_push_loop_reserve(
    ctx: &mut LowerCtx,
    init: Option<&Stmt>,
    cond: Option<ExprId>,
    step: Option<ExprId>,
    body: &Stmt,
) -> Vec<String> {
    let pushed_arrays = detect_push_loop_arrays(ctx.ast, init, cond, step, body);
    // Only a body whose every push argument is inert may keep the
    // length word in a register until the loop ends — see
    // `PreReserveState::defer_len`.
    let defer_len = crate::ssa_lower_push_loop_detect::push_args_all_inert(ctx, body);
    let mut reserve_emitted: Vec<String> = Vec::new();
    if let Some((bound_eid, names)) = &pushed_arrays {
        // The bound is read once here and trusted for the whole loop;
        // only some bounds may be. See `lower_reserve_bound`.
        let Some(bound_op) =
            crate::ssa_lower_push_loop_detect::lower_reserve_bound(ctx, *bound_eid, names)
        else {
            return reserve_emitted;
        };
        for name in names {
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
            // B1 — reserve never moves the cell.
            let state = ctx.emit_prereserved_state(reserved, defer_len);
            ctx.prereserve.unchecked_for.insert(name.clone(), state);
            reserve_emitted.push(name.clone());
        }
    }
    reserve_emitted
}
