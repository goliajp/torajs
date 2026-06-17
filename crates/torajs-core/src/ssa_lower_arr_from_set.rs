//! `Array.from(set)` / `[...set]` substrate (ES §23.1.2.1 + §24.2.3.13)
//! carved out of `ssa_lower.rs::lower_expr_inner` so both the
//! `Array.from(<set>)` arm and the array-literal `Expr::Spread` arm
//! can share one emit without duplicating the ~110-line map-iter loop.
//!
//! Set storage is hashmap-bucket tagged `(kt, kv)`; we walk it via the
//! shared `map_iter_next` runtime contract and push each tagged pair
//! into a fresh `Array<Any>` via `arr_push_any`. Heap-tagged entries
//! get `any_payload_rc_inc` so the new Array slot owns an independent
//! ref (the Set keeps its own).
//!
//! Returns the final `Type::Arr<Any>` operand ready for the caller to
//! consume — `Array.from` returns it directly; the spread arm passes
//! it on to the existing Arr-merge path.

use crate::ssa::{IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::{LowerCtx, intern_arr_layout};

/// Emit the Set → `Array<Any>` walk and return the resulting Arr<Any>
/// operand. Caller is responsible for receiver lowering (passes the
/// already-lowered `set_op`) and for consuming / dropping the result.
pub(crate) fn emit(ctx: &mut LowerCtx, set_op: Operand) -> Operand {
    let arr_id = intern_arr_layout(ctx.arr_layouts, Type::Any);
    let mut dst = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.arr_alloc_any, vec![Operand::ConstI64(0)]),
        Type::Arr(arr_id),
        None,
    );
    let dst_slot = ctx.alloca(Type::Arr(arr_id), Some("__from_set_dst"));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(dst), Operand::Value(dst_slot), 0),
    );
    let i_slot = ctx.alloca(Type::I64, Some("__from_set_i"));
    // -1 sentinel: signals "fresh" to map_iter_next (bucket cursor
    // initializes on first call). Same protocol as Set.forEach.
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::ConstI64(-1), Operand::Value(i_slot), 0),
    );
    let kt_slot = ctx.alloca(Type::I64, Some("__from_set_kt"));
    let kv_slot = ctx.alloca(Type::I64, Some("__from_set_kv"));
    let vt_slot = ctx.alloca(Type::I64, Some("__from_set_vt"));
    let vv_slot = ctx.alloca(Type::I64, Some("__from_set_vv"));
    let header_blk = ctx.f.add_block();
    let body_blk = ctx.f.add_block();
    let after_blk = ctx.f.add_block();
    ctx.f.set_term(ctx.cur_block, Terminator::Br(header_blk));
    ctx.cur_block = header_blk;
    let live = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.map_iter_next,
            vec![
                set_op.clone(),
                Operand::Value(i_slot),
                Operand::Value(kt_slot),
                Operand::Value(kv_slot),
                Operand::Value(vt_slot),
                Operand::Value(vv_slot),
            ],
        ),
        Type::I64,
        None,
    );
    let cond = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Ne, Operand::Value(live), Operand::ConstI64(0)),
        Type::Bool,
        None,
    );
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(cond),
            then_blk: body_blk,
            else_blk: after_blk,
        },
    );
    ctx.cur_block = body_blk;
    let kt_v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(kt_slot), 0),
        Type::I64,
        None,
    );
    let kv_v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(kv_slot), 0),
        Type::I64,
        None,
    );
    // Heap-tagged entries (Str / Obj / Arr) must rc_inc before being
    // copied — the Set keeps its own owning ref, so the new Array slot
    // needs an independent one. `any_payload_rc_inc` dispatches on tag
    // at runtime (tag == ANY_HEAP=4 → rc_inc value as ptr; other tags
    // → no-op), letting us skip the inline ICmp + branch.
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.any_payload_rc_inc,
            vec![Operand::Value(kt_v), Operand::Value(kv_v)],
        ),
    );
    let cur_dst = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::Arr(arr_id), Operand::Value(dst_slot), 0),
        Type::Arr(arr_id),
        None,
    );
    let new_dst = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.arr_push_any,
            vec![
                Operand::Value(cur_dst),
                Operand::Value(kt_v),
                Operand::Value(kv_v),
            ],
        ),
        Type::Arr(arr_id),
        None,
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(new_dst), Operand::Value(dst_slot), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(header_blk));
    ctx.cur_block = after_blk;
    dst = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::Arr(arr_id), Operand::Value(dst_slot), 0),
        Type::Arr(arr_id),
        None,
    );
    Operand::Value(dst)
}
