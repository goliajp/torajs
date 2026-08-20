//! RFC 20260714-dstr-residual blade 3 — array destructuring through
//! the iterator protocol.
//!
//! The parse-time desugar reads a binding pattern's elements by index
//! out of a `__ary_src_<id>` group temp. That is right for an Array and
//! wrong for every other iterable: ES §13.15.5.3 STEPS the source, so a
//! generator, a Map / Set, a class instance with `[Symbol.iterator]()`,
//! or any of those behind `any` never sees an index. Before this lane
//! an `any` source read `undefined` out of every slot (silently) and a
//! typed generator source was rejected outright by the checker.
//!
//! Rather than teach the pattern a second read shape, this lowers the
//! group temp itself: walk the source through the unified runtime
//! iteration protocol (`__torajs_any_iter_next` — the same kernel
//! `for-of` and spread drive) and bind the steps as an `Array<Any>`.
//! Every index read the desugar emitted then lands on a real array, so
//! defaults, elisions and `.slice(N)` rest all keep working unchanged.
//! The checker picks the lane (only it knows the source's type) and
//! records it in `ast.iter_destr_srcs`.
//!
//! **The walk is bounded**, which is the whole reason it isn't just a
//! spread. A pattern names a fixed number of elements, so it steps
//! exactly that many times: `const [a, b] = infinite()` terminates, and
//! a generator's side effects past the second yield don't happen. A
//! pattern WITH a rest element (`limit < 0`) drains to exhaustion, as
//! the rest binding must.
//!
//! Stopping early leaves the iterator un-exhausted, and ES §7.4.9 owes
//! it an IteratorClose — that is what runs a generator's `finally`
//! blocks. The `close` block calls the close kernel on exactly that
//! path; natural exhaustion skips it (a done iterator is not closed).

use crate::ast::ExprId;
use crate::ssa::{BinOp, IPred, InstKind, Operand, StructId, Terminator, Type};
use crate::ssa_lower::{LowerCtx, OBJ_HEADER_SIZE, intern_arr_layout};
use crate::ssa_lower_stmt_let_decl_general::bind_let_slot;

/// RFC 20260820-dstr-deferred-close — where a suspendable pattern's
/// walk parks its still-open iterator instead of closing it: the
/// desugar declared `__dstra_it_<id>` right before the group temp,
/// so the finally's `__torajs_dstr_close_pending` can reach it. A
/// pattern that drains its source to done leaves the park slot
/// `undefined` (a done iterator is not closed).
pub(crate) enum ParkTarget {
    /// The local `__dstra_it_<id>` alloca (non-generator context).
    Local(crate::ssa::ValueId),
    /// The lifted `this.__dstra_it_<id>` field (generator context).
    Field { obj: Operand, offset: u64 },
}

/// Resolve the park target the desugar paired with this group temp,
/// if any — `None` means the pattern cannot suspend and the walk
/// keeps its eager close.
fn park_for_local(ctx: &LowerCtx, src_name: &str) -> Option<ParkTarget> {
    let id = src_name.strip_prefix("__dstra_src_")?;
    let info = ctx.locals.get(&format!("__dstra_it_{id}"))?;
    Some(ParkTarget::Local(info.slot))
}

/// The lifted twin of [`park_for_local`]: both temps became fields
/// of the generator's state struct, so the park is a field store.
fn park_for_field(
    ctx: &LowerCtx,
    src_field: &str,
    obj: &Operand,
    sid: StructId,
) -> Option<ParkTarget> {
    let id = src_field.strip_prefix("__dstra_src_")?;
    let it_name = format!("__dstra_it_{id}");
    let layout = &ctx.struct_layouts[sid.0 as usize];
    layout.iter().enumerate().find_map(|(i, (n, _))| {
        (n == &it_name).then(|| ParkTarget::Field {
            obj: obj.clone(),
            offset: OBJ_HEADER_SIZE + (i as u64) * 8,
        })
    })
}

/// The `Stmt::LetDecl` sub-lane for an array-destructuring group temp
/// whose source needs the iterator protocol. Answers false — leaving
/// the binding to the general path — for every other declaration,
/// including the group temps whose source IS indexable.
pub(crate) fn try_lower_group(ctx: &mut LowerCtx, name: &str, init: ExprId) -> bool {
    let Some(&limit) = ctx.ast.iter_destr_srcs.get(&init) else {
        return false;
    };
    let park = park_for_local(ctx, name);
    let (recv, minted) = lower_src_to_any(ctx, init);
    let (arr, arr_ty) = emit_walk(ctx, recv.clone(), limit, park);
    // The kernel only borrows the receiver, so the stake that reached it
    // is still ours to settle. A box minted below owns one outright; a
    // pass-through `any` source owes a release only when the expression
    // that produced it was an owned temp (`gen()` — the borrow shapes
    // self-gate false inside `release_owned_temp`).
    if minted {
        ctx.emit_drop_value(recv, Type::Any);
    } else {
        ctx.release_owned_temp(init, &recv);
    }
    let cur_depth = ctx.scope_stack.len() - 1;
    bind_let_slot(ctx, name, arr_ty, arr, false, false, cur_depth);
    true
}

/// The struct-field twin of [`try_lower_group`] (rotation 455): a
/// generator lift rewrites the group temp's `LetDecl` into
/// `this.<temp> = init` before check runs, so the iterator lane has
/// to answer at the field-store site too — without this the lifted
/// temp held the raw source and every index read below it answered
/// `undefined` (the silent-wrong the lane exists to kill). Returns
/// the stepped `Array<Any>` boxed to `Any` — an OWNED stake the field
/// store takes verbatim (the box is a pure encode; the walk's fresh
/// array carries the +1).
pub(crate) fn try_lower_field_walk(
    ctx: &mut LowerCtx,
    init: ExprId,
    obj: &Operand,
    sid: StructId,
    field: &str,
) -> Option<Operand> {
    let &limit = ctx.ast.iter_destr_srcs.get(&init)?;
    // Bounded patterns only. A rest pattern (`limit < 0`) drains to
    // exhaustion, and in a generator that drain runs EAGERLY at the
    // lifted store — before any yield in the pattern's target could
    // suspend — so `[x, ...o[yield]] = infiniteIterable` (t262
    // rtrn-close rest family) would hang where the spec suspends
    // (§13.15.5.5 evaluates the rest TARGET's reference first).
    // Probe-proven this rotation. The rest family stays on its
    // pre-lane behaviour until the deferred-close redesign sequences
    // target evaluation before the drain.
    if limit < 0 {
        return None;
    }
    let park = park_for_field(ctx, field, obj, sid);
    let (recv, minted) = lower_src_to_any(ctx, init);
    let (arr, _arr_ty) = emit_walk(ctx, recv.clone(), limit, park);
    if minted {
        ctx.emit_drop_value(recv, Type::Any);
    } else {
        ctx.release_owned_temp(init, &recv);
    }
    Some(ctx.box_to_any(arr))
}

/// Lower the source expression and hand the kernel an AnyValue, saying
/// whether a box had to be minted for it. A concrete source (a
/// `Generator` binding, a `Set`) boxes: the box takes a reference of its
/// own, so a borrow-shaped init (an Ident) retains first and an owned
/// one (a `gen()` call) transfers the reference it already holds. An
/// `any` source is already in the kernel's shape and passes through.
fn lower_src_to_any(ctx: &mut LowerCtx, init: ExprId) -> (Operand, bool) {
    let src = ctx.lower_expr(init);
    let src_ty = ctx.operand_ty(&src);
    if src_ty == Type::Any {
        return (src, false);
    }
    if src_ty.is_refcounted() && !ctx.expr_transfers_ownership(init) {
        ctx.emit_rc_inc(src.clone());
    }
    (ctx.box_to_any_from_expr(init, src), true)
}

/// Deferred close's stop-short arm: transfer the iterator's stake
/// from the walk's local slot into the park target (its declared
/// init is `undefined` and the walk runs once per declaration, so no
/// drop-old is owed), then clear the local slot so the walk's shared
/// release no-ops. A suspendable pattern always has at least one
/// element, so the step block ran and the slot holds the derived
/// iterator — or stays `undefined` for a builtin-indexed lane, which
/// the pending-close kernel treats as nothing-to-close.
fn emit_park_transfer(ctx: &mut LowerCtx, iter_slot: crate::ssa::ValueId, target: ParkTarget) {
    let it = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::Any, Operand::Value(iter_slot), 0),
        Type::Any,
        None,
    );
    match target {
        ParkTarget::Local(slot) => {
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Store(Operand::Value(it), Operand::Value(slot), 0),
            );
        }
        ParkTarget::Field { obj, offset } => {
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Store(Operand::Value(it), obj, offset),
            );
        }
    }
    let undef = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.any_box,
            vec![Operand::ConstI64(5), Operand::ConstI64(0)],
        ),
        Type::Any,
        None,
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(undef), Operand::Value(iter_slot), 0),
    );
}

/// Emit the bounded iteration walk. `limit >= 0` steps at most that
/// many times and closes the iterator when it stops short; `limit < 0`
/// drains to exhaustion (a rest element consumes the tail).
///
/// With a `park` target the stop-short path PARKS the still-open
/// iterator there instead of closing it (deferred close — the
/// pattern's finally settles it); draining to done leaves the park
/// `undefined` and releases the iterator here as before.
fn emit_walk(
    ctx: &mut LowerCtx,
    recv: Operand,
    limit: i64,
    park: Option<ParkTarget>,
) -> (Operand, Type) {
    let arr_id = intern_arr_layout(ctx.arr_layouts, Type::Any);
    let arr_ty = Type::Arr(arr_id);
    let dst = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.arr_alloc_any, vec![Operand::ConstI64(0)]),
        arr_ty,
        None,
    );
    let dst_slot = ctx.alloca(arr_ty, Some("__dstr_iter_dst"));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(dst), Operand::Value(dst_slot), 0),
    );
    let idx_slot = ctx.alloca(Type::I64, Some("__dstr_iter_idx"));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::ConstI64(0), Operand::Value(idx_slot), 0),
    );
    let cnt_slot = ctx.alloca(Type::I64, Some("__dstr_iter_cnt"));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::ConstI64(0), Operand::Value(cnt_slot), 0),
    );
    let iter_slot = crate::ssa_lower_for_of_any_iter::emit_iter_slot(ctx, "__dstr_iter_iter");
    let out_slot = ctx.alloca(Type::Any, Some("__dstr_iter_out"));

    let guard_blk = ctx.f.add_block();
    let step_blk = ctx.f.add_block();
    let body_blk = ctx.f.add_block();
    let close_blk = ctx.f.add_block();
    let after_blk = ctx.f.add_block();
    ctx.f.set_term(ctx.cur_block, Terminator::Br(guard_blk));

    // The pattern's step budget. A rest element makes it unbounded, and
    // the guard collapses to a straight edge into the step.
    ctx.cur_block = guard_blk;
    if limit >= 0 {
        let cnt = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(Type::I64, Operand::Value(cnt_slot), 0),
            Type::I64,
            None,
        );
        let full = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::ICmp(IPred::Sge, Operand::Value(cnt), Operand::ConstI64(limit)),
            Type::Bool,
            None,
        );
        ctx.f.set_term(
            ctx.cur_block,
            Terminator::CondBr {
                cond: Operand::Value(full),
                then_blk: close_blk,
                else_blk: step_blk,
            },
        );
    } else {
        ctx.f.set_term(ctx.cur_block, Terminator::Br(step_blk));
    }

    ctx.cur_block = step_blk;
    let live = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.any_iter_next,
            vec![
                recv.clone(),
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

    // Each step's out-slot holds an OWNED AnyValue; unboxing to the
    // (tag, value) pair and pushing transfers that stake into the array
    // slot, so no rc traffic is emitted (the `arr_from_any` ledger).
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
        InstKind::Load(arr_ty, Operand::Value(dst_slot), 0),
        arr_ty,
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
        arr_ty,
        None,
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(new_dst), Operand::Value(dst_slot), 0),
    );
    let cnt = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(cnt_slot), 0),
        Type::I64,
        None,
    );
    let next_cnt = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::BinOp(BinOp::Add, Operand::Value(cnt), Operand::ConstI64(1)),
        Type::I64,
        None,
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(next_cnt), Operand::Value(cnt_slot), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(guard_blk));

    // Budget spent with the iterator still live — ES §7.4.9. The kernel
    // also validates iterability here for a zero-step pattern
    // (`const [] = src` never reaches the step block at all).
    ctx.cur_block = close_blk;
    match park {
        None => {
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Call(
                    ctx.intrinsics.any_iter_close,
                    vec![recv, Operand::Value(iter_slot)],
                ),
            );
            ctx.emit_throw_check(None);
        }
        Some(target) => emit_park_transfer(ctx, iter_slot, target),
    }
    ctx.f.set_term(ctx.cur_block, Terminator::Br(after_blk));

    ctx.cur_block = after_blk;
    crate::ssa_lower_for_of_any_iter::emit_iter_slot_release(ctx, iter_slot);
    let final_dst = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(arr_ty, Operand::Value(dst_slot), 0),
        arr_ty,
        None,
    );
    (Operand::Value(final_dst), arr_ty)
}
