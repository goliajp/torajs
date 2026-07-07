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
//!   count. Restores `push_unchecked_for` to its pre-loop state.
//! - Drop init-scope locals + restore shadowed bindings.

use crate::ast::{ExprId, Stmt};
use crate::ssa::{BinOp as SsaBinOp, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx, PreReserveState};
use crate::ssa_lower_push_loop_detect::detect_push_loop_arrays;

pub(crate) fn lower(
    ctx: &mut LowerCtx,
    init: Option<&Stmt>,
    cond: Option<ExprId>,
    step: Option<ExprId>,
    body: &Stmt,
) {
    ctx.scope_stack.push(Vec::new());
    ctx.shadow_stack.push(Vec::new());
    if let Some(i) = init {
        ctx.lower_stmt(i);
    }
    let pushed_arrays = detect_push_loop_arrays(ctx.ast, init, cond, step, body);
    let mut reserve_emitted: Vec<String> = Vec::new();
    if let Some((bound_eid, names)) = &pushed_arrays {
        let bound_op = ctx.lower_expr(*bound_eid);
        for name in names {
            let Some(info) = ctx.locals.get(name).copied() else {
                continue;
            };
            if !matches!(info.ty, Type::Arr(_)) {
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
            // B1 — reserve never moves the cell; write-back retired.
            let head_x8 = ctx.emit_arr_head_x8(Operand::Value(reserved));
            let head_off = match head_x8 {
                Operand::Value(v) => v,
                _ => unreachable!("emit_arr_head_x8 returns a value"),
            };
            let data_op = ctx.emit_arr_data_ptr(Operand::Value(reserved));
            let data_ptr = match data_op {
                Operand::Value(v) => v,
                _ => unreachable!("emit_arr_data_ptr returns a value"),
            };
            let len_after = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Load(Type::I64, Operand::Value(reserved), ARR_LEN_OFF),
                Type::I64,
                None,
            );
            let len_slot = ctx.alloca(Type::I64, Some("__push_len"));
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Store(Operand::Value(len_after), Operand::Value(len_slot), 0),
            );
            ctx.push_unchecked_for.insert(
                name.clone(),
                PreReserveState {
                    arr_ptr: reserved,
                    data_ptr,
                    head_off,
                    len_slot,
                },
            );
            reserve_emitted.push(name.clone());
        }
    }
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
    ctx.lower_stmt(body);
    if ctx.cur_open() {
        ctx.f.set_term(ctx.cur_block, Terminator::Br(step_blk));
    }
    ctx.loop_stack.pop();

    ctx.cur_block = step_blk;
    if let Some(eid) = step {
        let _ = ctx.lower_expr(eid);
    }
    if ctx.cur_open() {
        ctx.f.set_term(ctx.cur_block, Terminator::Br(header));
    }

    ctx.cur_block = after;
    for name in &reserve_emitted {
        if let Some(state) = ctx.push_unchecked_for.get(name).copied() {
            let final_len = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Load(Type::I64, Operand::Value(state.len_slot), 0),
                Type::I64,
                None,
            );
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Store(
                    Operand::Value(final_len),
                    Operand::Value(state.arr_ptr),
                    ARR_LEN_OFF,
                ),
            );
        }
    }
    for name in &reserve_emitted {
        ctx.push_unchecked_for.remove(name);
    }
    let frame = ctx.scope_stack.pop().expect("for-init scope");
    let shadows = ctx.shadow_stack.pop().expect("shadow frame");
    for name in &frame {
        let info = match ctx.locals.get(name) {
            Some(i) => *i,
            None => continue,
        };
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
        ctx.locals.remove(&name);
    }
    for (name, prev) in shadows {
        ctx.locals.insert(name, prev);
    }
}
