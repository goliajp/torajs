//! `LowerCtx::lower_for_of_map_like` extracted from
//! [`crate::ssa_lower::LowerCtx`] (chunk 137).
//!
//! Pre-extract this method was 254 LOC inline on `LowerCtx` (over
//! the 200-line god-fn hard limit per `torajs-file-size-debt`).
//! Body verbatim moved here as a free fn taking `&mut LowerCtx`;
//! the call-site in `ssa_lower.rs`'s `Stmt::ForOf` lowering arm
//! delegates with one line.
//!
//! Lowers a `for-of` over a Map / Set / MapIter / ArrIter source:
//! - Map's default iter = `entries()` per spec §23.1.4. We mint a
//!   fresh MapIter and the loop var ends up `Type::Arr<Any>`
//!   ([key, value] pair re-loaded directly as `Arr<Any>` because
//!   the val_slot's i64 happens to be the entries-array ptr).
//! - Set's default iter = `values()` (same as keys since storage's
//!   value side is `ANY_UNDEF`) per spec §24.2.5.1. Loop var =
//!   `Type::Any`.
//! - MapIter / ArrIter sources reuse the caller's iter — we don't
//!   mint a fresh one and we don't drop it at `after_blk`. Loop var
//!   = `Type::Any`.
//!
//! Map / Set / MapIter route through `__torajs_map_iter_step`;
//! ArrIter routes through `__torajs_arr_iter_step` (same `(tag,
//! payload)` out-pair contract). Body iteration:
//!
//! 1. Get / create iter + decide `iter_ty`, `step_fid`,
//!    `should_drop_iter`, loop var type.
//! 2. Stash iter into an alloca slot for uniform per-iter Load.
//! 3. Pre-alloc `(tag, payload)` out-slots for the step call.
//! 4. Header — call step; exit on `live == 0`.
//! 5. Body — Load value (Arr<Any> direct for Map; any_box for
//!    Set/MapIter/ArrIter), alloca-bind `var_name`, lower body,
//!    drop owned locals at iteration end (Arr<Any> dispatches
//!    `arr_drop`, Any dispatches `any_box_drop`).
//! 6. After — drop iter if we minted it.

use crate::ast::Stmt;
use crate::ssa::{FuncId, IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::{LowerCtx, intern_arr_layout};
use crate::ssa_lower_stmt_for_of::{bind_scoped_local, close_body_scope};

/// Step 1 — get / create the iterator and decide the loop shape:
/// `(iter_op, should_drop_iter, var_ty, iter_ty, step_fid)`. Map
/// mints a fresh `entries()` MapIter (spec §23.1.4; loop var =
/// `Arr<Any>`), Set mints `keys()` (§24.2.5.1; loop var = `Any`);
/// MapIter / ArrIter reuse the caller's iter without a mint (no
/// drop at `after_blk`), loop var = `Any`.
fn resolve_map_like_iter(
    ctx: &mut LowerCtx,
    src_op: Operand,
    src_ty: Type,
) -> (Operand, bool, Type, Type, FuncId) {
    match src_ty {
        Type::Map => {
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.map_iter_create_entries, vec![src_op]),
                Type::MapIter,
                None,
            );
            let arr_id = intern_arr_layout(ctx.arr_layouts, Type::Any);
            (
                Operand::Value(v),
                true,
                Type::Arr(arr_id),
                Type::MapIter,
                ctx.intrinsics.map_iter_step,
            )
        }
        Type::Set => {
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.map_iter_create_keys, vec![src_op]),
                Type::MapIter,
                None,
            );
            (
                Operand::Value(v),
                true,
                Type::Any,
                Type::MapIter,
                ctx.intrinsics.map_iter_step,
            )
        }
        Type::MapIter => (
            src_op,
            false,
            Type::Any,
            Type::MapIter,
            ctx.intrinsics.map_iter_step,
        ),
        Type::ArrIter => (
            src_op,
            false,
            Type::Any,
            Type::ArrIter,
            ctx.intrinsics.arr_iter_step,
        ),
        _ => unreachable!("lower_for_of_map_like: src_ty must be Map | Set | MapIter | ArrIter"),
    }
}

/// Step 5 (value) — Load the per-iteration loop value at the top of
/// the body block. Map re-loads the val_slot directly as `Arr<Any>`
/// (the i64 happens to be the entries-array ptr); Set / MapIter /
/// ArrIter box the `(tag, payload)` out-pair via `any_box`.
fn load_loop_value(
    ctx: &mut LowerCtx,
    src_ty: Type,
    var_ty: Type,
    tag_slot: ValueId,
    val_slot: ValueId,
) -> ValueId {
    match src_ty {
        Type::Map => ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(var_ty, Operand::Value(val_slot), 0),
            var_ty,
            None,
        ),
        Type::Set | Type::MapIter | Type::ArrIter => {
            let tag_v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Load(Type::I64, Operand::Value(tag_slot), 0),
                Type::I64,
                None,
            );
            let pv = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Load(Type::I64, Operand::Value(val_slot), 0),
                Type::I64,
                None,
            );
            ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(
                    ctx.intrinsics.any_box,
                    vec![Operand::Value(tag_v), Operand::Value(pv)],
                ),
                Type::Any,
                None,
            )
        }
        _ => unreachable!(),
    }
}

pub(crate) fn lower_for_of_map_like(
    ctx: &mut LowerCtx,
    src_op: Operand,
    src_ty: Type,
    var_name: &str,
    body: &Stmt,
) {
    let (iter_op, should_drop_iter, var_ty, iter_ty, step_fid) =
        resolve_map_like_iter(ctx, src_op, src_ty);

    ctx.scope_stack.push(Vec::new());
    ctx.shadow_stack.push(Vec::new());
    let iter_slot = ctx.alloca(iter_ty, Some("__forof_map_it"));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(iter_op, Operand::Value(iter_slot), 0),
    );

    let tag_slot = ctx.alloca(Type::I64, Some("__forof_map_tag"));
    let val_slot = ctx.alloca(Type::I64, Some("__forof_map_val"));

    let header = ctx.f.add_block();
    let body_blk = ctx.f.add_block();
    let after = ctx.f.add_block();
    ctx.f.set_term(ctx.cur_block, Terminator::Br(header));

    ctx.cur_block = header;
    let iter_load = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(iter_ty, Operand::Value(iter_slot), 0),
        iter_ty,
        None,
    );
    let live = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            step_fid,
            vec![
                Operand::Value(iter_load),
                Operand::Value(tag_slot),
                Operand::Value(val_slot),
            ],
        ),
        Type::I64,
        None,
    );
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
            then_blk: after,
            else_blk: body_blk,
        },
    );

    ctx.cur_block = body_blk;
    ctx.scope_stack.push(Vec::new());
    ctx.shadow_stack.push(Vec::new());
    let v_val = load_loop_value(ctx, src_ty, var_ty, tag_slot, val_slot);
    let v_slot = ctx.alloca(var_ty, Some(var_name));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(v_val), Operand::Value(v_slot), 0),
    );
    bind_scoped_local(ctx, var_name, v_slot, var_ty, false, false);
    // RFC 20260901-scope-exit-drops — body frame already pushed and
    // only closed on fall-through: a jump out owes it (depth = index).
    ctx.loop_stack.push(crate::ssa_lower_scope_exit::LoopTargets {
        cont: header,
        brk: after,
        scope_depth: ctx.scope_stack.len() - 1,
    });
    ctx.lower_stmt(body);
    let body_open = ctx.cur_open();
    ctx.loop_stack.pop();
    close_body_scope(ctx, header, body_open);

    ctx.cur_block = after;
    if should_drop_iter {
        let iter_load_drop = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(iter_ty, Operand::Value(iter_slot), 0),
            iter_ty,
            None,
        );
        ctx.emit_drop_value(Operand::Value(iter_load_drop), iter_ty);
    }
    let _ = ctx.scope_stack.pop().expect("for-of-map iter scope");
    let _ = ctx.shadow_stack.pop().expect("shadow frame");
}
