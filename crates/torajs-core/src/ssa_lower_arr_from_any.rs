//! `[...src]` where `src` lowers to `Type::Any` — materialize the
//! runtime-dispatched iterable into a fresh `Array<Any>` (RFC
//! 20260704 S5+, the spread consumer of the unified iteration
//! protocol).
//!
//! Mirrors `ssa_lower_arr_from_set` (the `[...set]` walk): a
//! `__torajs_any_iter_next` loop drives whatever shape hides behind
//! the erased type (strings / arrays / MapIter / ArrIter cells;
//! non-iterables raise a catchable TypeError propagated by the
//! per-step throw check). Each step's out-slot holds an OWNED
//! AnyValue — unboxing to the `(tag, value)` pair and handing it to
//! `arr_push_any` transfers that ownership into the array slot, so
//! no rc traffic is emitted (contrast from_set, whose step yields
//! borrows that need `any_payload_rc_inc`).
//!
//! Returns a fresh rc=1 `Arr<Any>` operand — an owned temp the
//! caller must consume or drop (the spread arms extend it into the
//! literal array and drop it).
//!
//! Also hosts `assemble_any_spread` — the Array<Any> literal
//! assembler over ALREADY-lowered spread/literal items (moved from
//! `ssa_lower_array` by the 500-line file discipline; replaces the
//! pre-S5+ ExprId re-lowering shape that double-emitted spread
//! source side effects).

use crate::ssa::{IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::{LowerCtx, intern_arr_layout};
use crate::ssa_lower_array_spread::LoweredItem;

/// Emit the any-iterable → `Array<Any>` materialization loop. See
/// module doc for the ownership contract.
///
/// A non-iterable source raises a catchable TypeError, which is what
/// `[...x]` means.
pub(crate) fn emit(ctx: &mut LowerCtx, src_op: Operand) -> Operand {
    let step_fn = ctx.intrinsics.any_iter_next;
    emit_with_step(ctx, src_op, step_fn)
}

/// The same loop for `Array.from`, whose §23.1.2.1 step 3 walks
/// `length` and the index keys instead of throwing when the source
/// turns out to have no iterator. The two entries differ ONLY in that
/// verdict — the walk, the ownership contract and the resulting array
/// are identical — so the branch lives in the kernel rather than in a
/// second loop here.
pub(crate) fn emit_for_array_from(ctx: &mut LowerCtx, src_op: Operand) -> Operand {
    let step_fn = ctx.intrinsics.any_iter_next_array_like;
    emit_with_step(ctx, src_op, step_fn)
}

fn emit_with_step(ctx: &mut LowerCtx, src_op: Operand, step_fn: crate::ssa::FuncId) -> Operand {
    let arr_id = intern_arr_layout(ctx.arr_layouts, Type::Any);
    let dst = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.arr_alloc_any, vec![Operand::ConstI64(0)]),
        Type::Arr(arr_id),
        None,
    );
    let dst_slot = ctx.alloca(Type::Arr(arr_id), Some("__from_any_dst"));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(dst), Operand::Value(dst_slot), 0),
    );
    let idx_slot = ctx.alloca(Type::I64, Some("__from_any_idx"));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::ConstI64(0), Operand::Value(idx_slot), 0),
    );
    let iter_slot = crate::ssa_lower_for_of_any_iter::emit_iter_slot(ctx, "__from_any_iter");
    let out_slot = ctx.alloca(Type::Any, Some("__from_any_out"));

    let header_blk = ctx.f.add_block();
    let body_blk = ctx.f.add_block();
    let after_blk = ctx.f.add_block();
    ctx.f.set_term(ctx.cur_block, Terminator::Br(header_blk));

    ctx.cur_block = header_blk;
    let live = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            step_fn,
            vec![
                src_op,
                Operand::Value(idx_slot),
                Operand::Value(iter_slot),
                Operand::Value(out_slot),
            ],
        ),
        Type::I64,
        None,
    );
    ctx.emit_throw_check(None);
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
    let out_v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::Any, Operand::Value(out_slot), 0),
        Type::Any,
        None,
    );
    let tag_v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.any_unbox_tag, vec![Operand::Value(out_v)]),
        Type::I64,
        None,
    );
    let val_v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.any_unbox_value, vec![Operand::Value(out_v)]),
        Type::I64,
        None,
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
                Operand::Value(tag_v),
                Operand::Value(val_v),
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
    crate::ssa_lower_for_of_any_iter::emit_iter_slot_release(ctx, iter_slot);
    let final_dst = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::Arr(arr_id), Operand::Value(dst_slot), 0),
        Type::Arr(arr_id),
        None,
    );
    Operand::Value(final_dst)
}

/// Assemble an `Array<Any>` literal from ALREADY-lowered items (RFC
/// 20260704 S5+). Literal items pack via the shared `pack_any_elem`
/// (rc_inc / unbox ledger inside); spread items are `Arr<Any>` here
/// by construction (`any` sources were materialized by
/// `lower_spread_source`) and extend-copy with per-slot rc_inc —
/// materialized temps drop right after their extend.
pub(crate) fn assemble_any_spread(
    ctx: &mut LowerCtx<'_>,
    lowered: Vec<LoweredItem>,
    literal_count: i64,
) -> Operand {
    let arr_id = intern_arr_layout(ctx.arr_layouts, Type::Any);
    // Fast path: `[...anyval]` alone — the materialized Arr<Any> IS
    // the literal (fresh rc=1, nothing else to splice).
    if let [li] = lowered.as_slice()
        && li.is_spread
        && li.was_any
    {
        return li.op.clone();
    }
    let mut arr = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.arr_alloc_any,
            vec![Operand::ConstI64(literal_count)],
        ),
        Type::Arr(arr_id),
        None,
    );
    for li in lowered {
        if li.is_spread {
            let src_is_any_arr = match ctx.operand_ty(&li.op) {
                Type::Arr(src_arr_id) => {
                    matches!(ctx.arr_layouts[src_arr_id.0 as usize], Type::Any)
                }
                _ => false,
            };
            if !src_is_any_arr {
                // Rotation 543 — this used to be a loud reject citing
                // a follow-up ("typed-Array spread into Any requires
                // per-elem box"). That follow-up landed on the runtime
                // side and nobody came back for the reject:
                // `__torajs_arr_extend_any` reads the source header's
                // element kind and routes a typed block through
                // `__torajs_arr_extend_typed_into_any`, which boxes
                // per slot, honours Bool's 1-byte stride, rc_incs each
                // heap cell and materializes an inline Substr view.
                // All this arm owes it is the kind mark.
                let src_ty = ctx.operand_ty(&li.op);
                let chain = match src_ty {
                    Type::Arr(id) => {
                        let elem = ctx.arr_layouts[id.0 as usize];
                        ctx.arr_kind_chain(&elem, 0)
                    }
                    _ => 0,
                };
                if chain == 0 {
                    panic!(
                        "ssa-lower: spread of {src_ty:?} into Array<Any> literal has no element kind to mark — the runtime bridge needs one to box per slot"
                    );
                }
                ctx.emit_arr_mark_kind(&li.op);
            }
            let new_arr = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(
                    ctx.intrinsics.arr_extend_any,
                    vec![Operand::Value(arr), li.op.clone()],
                ),
                Type::Arr(arr_id),
                None,
            );
            // Rotation 543 — `minted` reaches here too: a Set source
            // walks into a fresh `Arr<Any>`, which makes the literal
            // Any-typed and routes it through this assembler rather
            // than the typed one. It has no other owner either.
            if li.was_any || li.minted {
                let src_ty = ctx.operand_ty(&li.op);
                ctx.emit_drop_value(li.op, src_ty);
            } else {
                // Rotation 543 — the extend copies the slots and takes
                // its own +1 per heap cell, so a source the program can
                // still name owes the same account the typed assembler
                // now keeps: released when it was an owned temp,
                // untouched when it was a borrow.
                ctx.release_owned_temp(li.src_eid, &li.op);
            }
            arr = new_arr;
        } else {
            let v_ty = ctx.operand_ty(&li.op);
            let (tag_op, value_op) = ctx.pack_any_elem(li.op, v_ty, Some(li.src_eid));
            arr = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(
                    ctx.intrinsics.arr_push_any,
                    vec![Operand::Value(arr), tag_op, value_op],
                ),
                Type::Arr(arr_id),
                None,
            );
        }
    }
    Operand::Value(arr)
}
