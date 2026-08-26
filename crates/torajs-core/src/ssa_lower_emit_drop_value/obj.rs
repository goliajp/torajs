//! `Type::Obj(sid)` drop sequence — the inline `if (val != null) {
//! if (--rc == 0) { walk_fields; free } }` with the V3-05
//! self-referential guard and the T-26 cycle-collector hooks for
//! class sids — split out of `ssa_lower_emit_drop_value.rs` in
//! rotation 470 when the four inline gates landed (WeakRef observer
//! count / expando NULL / FLAG_BUFFERED / leaf-layout buffer skip).
//! The parent keeps the per-type dispatch and the shared
//! `nullish_skip_cond`; this child reaches it via `super::`.

use crate::ssa::{BinOp as SsaBinOp, FuncId, IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::{LowerCtx, OBJ_HEADER_SIZE, OBJ_PROPS_OFF};

use super::nullish_skip_cond;

/// `Type::Obj(sid)` drop — inline `if (val != null) { if (--rc == 0)
/// { walk_fields; free } }` with the V3-05 self-referential guard
/// (recursive same-sid children route through `value_drop_heap`) and
/// the T-26 cycle-collector hooks for class sids.
pub(super) fn emit_drop_obj(ctx: &mut LowerCtx, val: Operand, sid: crate::ssa::StructId) {
    if ctx.drop_inline_stack.contains(&sid.0) {
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.value_drop_heap, vec![val]),
        );
        return;
    }
    ctx.drop_inline_stack.insert(sid.0);
    let layout = ctx.struct_layouts[sid.0 as usize].clone();
    let dec_blk = ctx.f.add_block();
    let walk_blk = ctx.f.add_block();
    let after = ctx.f.add_block();
    let skip_check = nullish_skip_cond(ctx, &val);
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: skip_check,
            then_blk: after,
            else_blk: dec_blk,
        },
    );
    ctx.cur_block = dec_blk;
    let rc_new = ctx.emit_rc_dec_inline(val);
    let is_zero = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Eq, rc_new, Operand::ConstI32(0)),
        Type::Bool,
        None,
    );
    let is_class_sid = ctx
        .ast
        .class_parents
        .keys()
        .any(|cn| matches!(ctx.aliases.get(cn), Some(Type::Obj(s)) if s.0 == sid.0));
    let buffer_blk = if is_class_sid {
        ctx.f.add_block()
    } else {
        after
    };
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(is_zero),
            then_blk: walk_blk,
            else_blk: buffer_blk,
        },
    );
    if is_class_sid {
        ctx.cur_block = buffer_blk;
        // Rotation 470 — an instance whose declared fields are all
        // scalars can only sit on a cycle through its expando
        // sidecar; with that NULL too it is a leaf and never a
        // candidate root, so the buffer call is skipped inline. The
        // runtime probe (`has_walkable_children`) would have said
        // yes to any class instance.
        if layout.iter().all(|(_, fty)| fty.is_copy()) {
            let props_val = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Load(Type::Ptr, val, OBJ_PROPS_OFF),
                Type::Ptr,
                None,
            );
            // r502 — a link seam (`__torajs_obj_buffer_slow`, default =
            // `__torajs_cycle_buffer`) guarded on the bag's one attach
            // entry: no attach live, no bag, no cycle to buffer.
            emit_if_nonnull_call_arg(ctx, props_val, ctx.intrinsics.obj_buffer_slow, val);
        } else {
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.cycle_buffer, vec![val]),
            );
        }
        ctx.f.set_term(ctx.cur_block, Terminator::Br(after));
    }
    ctx.cur_block = walk_blk;
    if is_class_sid {
        // Rotation 470 — only notify the WeakRef registry while
        // something is actually observing: read the live-observer
        // count `torajs-rc` exports (the same gate its own `dec_ref`
        // applies, 469 A1) instead of paying a cross-archive call per
        // dying instance in a program that never says `WeakRef`.
        let active_ptr = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::GlobalRef(crate::ssa::WEAKREF_ACTIVE_SYM.to_string()),
            Type::Ptr,
            None,
        );
        let active = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(Type::I64, Operand::Value(active_ptr), 0),
            Type::I64,
            None,
        );
        let observed = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::ICmp(IPred::Ne, Operand::Value(active), Operand::ConstI64(0)),
            Type::Bool,
            None,
        );
        let notify_blk = ctx.f.add_block();
        let cont_blk = ctx.f.add_block();
        ctx.f.set_term(
            ctx.cur_block,
            Terminator::CondBr {
                cond: Operand::Value(observed),
                then_blk: notify_blk,
                else_blk: cont_blk,
            },
        );
        ctx.cur_block = notify_blk;
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.weakref_target_dying, vec![val]),
        );
        ctx.f.set_term(ctx.cur_block, Terminator::Br(cont_blk));
        ctx.cur_block = cont_blk;
    }
    for (i, (_, fty)) in layout.iter().enumerate() {
        if fty.is_copy() {
            continue;
        }
        let offset = OBJ_HEADER_SIZE + i as u64 * 8;
        let field_val =
            ctx.f
                .append_inst(ctx.cur_block, InstKind::Load(*fty, val, offset), *fty, None);
        ctx.emit_drop_value(Operand::Value(field_val), *fty);
    }
    // Props dynobj @ +24 (RFC 20260714-struct-dynamic-props blade 1)
    // — release the lazily-allocated expando shadow. value_drop_heap
    // is NULL-safe, so the common no-expando case costs one load and
    // an immediately-returning call (the cycle walker's per-child
    // convention, torajs-cycle/obj_drop.rs).
    //
    // Rotation 470 — both the expando release and the cycle-buffer
    // scrub are gated inline on the bit that decides them (a NULL
    // sidecar / a clear FLAG_BUFFERED), so the common instance —
    // never expanded, never buffered — makes neither call.
    let props_val = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::Ptr, val, OBJ_PROPS_OFF),
        Type::Ptr,
        None,
    );
    // r502 — the bag release is a link seam (`__torajs_obj_props_
    // drop_slow`, default = the generic value drop of the bag) guarded
    // on the bag's one attach entry.
    emit_if_nonnull_call_arg(ctx, props_val, ctx.intrinsics.obj_props_drop_slow, val);
    if is_class_sid {
        // r502 — `__torajs_obj_unbuffer_slow` (default = `cycle_
        // unbuffer`) guarded on `__torajs_cycle_buffer`: only it sets
        // FLAG_BUFFERED.
        emit_if_flag_call(
            ctx,
            val,
            torajs_rc::FLAG_BUFFERED,
            ctx.intrinsics.obj_unbuffer_slow,
        );
    }
    let obj_block_size = OBJ_HEADER_SIZE + (layout.len() as u64) * 8;
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.obj_drop_sized,
            vec![val, Operand::ConstI64(obj_block_size as i64)],
        ),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(after));
    ctx.cur_block = after;
    ctx.drop_inline_stack.remove(&sid.0);
}

/// `if (ptr != NULL) fid(ptr)` — leaves `cur_block` on the join.
/// `if (ptr != NULL) fid(arg)` — leaves `cur_block` on the join.
fn emit_if_nonnull_call_arg(ctx: &mut LowerCtx, ptr: ValueId, fid: FuncId, arg: Operand) {
    let nonnull = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Ne, Operand::Value(ptr), Operand::ConstPtrNull),
        Type::Bool,
        None,
    );
    emit_cond_call(ctx, nonnull, fid, arg);
}

/// `if (hdr.flags & flag) fid(hdr)` — the u16 `flags` at +6 read as
/// the high half of the u32 at +4; leaves `cur_block` on the join.
fn emit_if_flag_call(ctx: &mut LowerCtx, hdr: Operand, flag: u16, fid: FuncId) {
    let word = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I32, hdr.clone(), 4),
        Type::I32,
        None,
    );
    let masked = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::BinOp(
            SsaBinOp::And,
            Operand::Value(word),
            Operand::ConstI32(((flag as u32) << 16) as i32),
        ),
        Type::I32,
        None,
    );
    let set = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Ne, Operand::Value(masked), Operand::ConstI32(0)),
        Type::Bool,
        None,
    );
    emit_cond_call(ctx, set, fid, hdr);
}

fn emit_cond_call(ctx: &mut LowerCtx, cond: ValueId, fid: FuncId, arg: Operand) {
    let call_blk = ctx.f.add_block();
    let join_blk = ctx.f.add_block();
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(cond),
            then_blk: call_blk,
            else_blk: join_blk,
        },
    );
    ctx.cur_block = call_blk;
    ctx.f
        .append_void(ctx.cur_block, InstKind::Call(fid, vec![arg]));
    ctx.f.set_term(ctx.cur_block, Terminator::Br(join_blk));
    ctx.cur_block = join_blk;
}
