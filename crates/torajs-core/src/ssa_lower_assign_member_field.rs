//! The typed-receiver tail of the member-assign ladder — the P8.2
//! accessor-setter direct call and the direct struct field store —
//! moved out of [`crate::ssa_lower_assign_member`] when the rotation
//! 240 setter-argument fix pushed that file past the 500-line limit
//! (the rotation 230 watch called this exact split).

mod fit;
pub(crate) mod guard;
mod rhs;

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, StructId, Type};
use crate::ssa_lower::{LowerCtx, OBJ_HEADER_SIZE};

use rhs::{Rhs, store_rhs};

pub(crate) fn try_lower_setter_call(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    obj: ExprId,
    obj_val: Operand,
    _sid: StructId,
    field: &str,
    value: ExprId,
) -> Option<Operand> {
    // P8.2 — accessor write `c.value = v`: call the setter
    // `__cm_<C>__<name>_set` (recorded in `ast.accessor_setters`) with
    // `[obj_val, value]` and evaluate to the value. The setter's class
    // comes from the receiver's NAME, not its layout id, and may be an
    // ANCESTOR (the [[Set]] walk starts from the full ClassRef key); a
    // generic declarer's setter is reached through the mono retarget
    // the checker recorded for this site.
    use crate::ssa_lower_member_obj_field::accessor_slot;
    let cls_key = accessor_slot::class_key_of(ctx, obj)?;
    let hit = crate::ast::accessor_lookup::accessor_setter_in_chain(ctx.ast, &cls_key, field)?;
    let fid = match ctx.fn_table.get(&hit.fn_name).copied() {
        Some(f) => f,
        None => {
            let mono = ctx.call_retargets.get(&eid)?;
            ctx.fn_table.get(mono).copied()?
        }
    };
    let v = ctx.lower_expr(value);
    // Chunk 566 — SHARE: no consume. The value passes to the setter
    // as a +0 borrow; the setter body's own field store takes the
    // field's +1 (struct-field share below).
    let transfers = ctx.expr_transfers_ownership(value);
    let mut owned: Vec<(Operand, Type)> = Vec::new();
    let args = setter_args(ctx, fid, &hit.fn_name, obj_val, (value, v), &mut owned);
    let recv = obj_val;
    let slot = accessor_slot::find(ctx, &cls_key, field, "_set");
    let call = move |ctx: &mut LowerCtx<'_>| {
        if let Some(slot) = &slot {
            accessor_slot::emit_call(ctx, slot, obj, fid, &hit.fn_name, args);
            return;
        }
        let cur_block = ctx.cur_block;
        ctx.f.append_void(cur_block, InstKind::Call(fid, args));
        ctx.emit_throw_check(Some(fid));
    };
    if ctx.ast.program_reparents {
        // RFC 20260920-struct-user-proto K4 — a re-parented instance
        // writes through its user chain's setter.
        crate::ssa_lower_call_reparent_guard::guarded_setter(ctx, recv, field, v, call);
    } else {
        call(ctx);
    }
    for (op, ty) in owned {
        ctx.emit_drop_value(op, ty);
    }
    // The setter borrowed the value; the assignment's consumer gets the
    // owned stake, so a non-storing setter no longer leaves a kept result
    // pointing at a released temp.
    let v_ty = ctx.operand_ty(&v);
    Some(crate::ssa_lower_assign_member::finish_assign_value(
        ctx, eid, v, v_ty, transfers,
    ))
}

/// The setter call's argument list for the lowered `value`. F2-fix,
/// generalized (rotation 240) — this is a direct Call, so the argument
/// crosses the same lane boundary as any direct call's: the untyped
/// setter (`set p(v)` — Any param) handed an i64 rhs verbatim stored
/// raw bits as a NaN-box (p25g SIGSEGV / silent no-output), so the
/// value goes through the one `arg_conv` contract.
fn setter_args(
    ctx: &mut LowerCtx<'_>,
    fid: crate::ssa::FuncId,
    fn_name: &str,
    obj_val: Operand,
    (value, v): (ExprId, Operand),
    owned: &mut Vec<(Operand, Type)>,
) -> Vec<Operand> {
    use crate::ssa_lower_member_obj_field::accessor_argv as acc;
    let arg = match acc::class_setter_value_ty(ctx, fid, fn_name) {
        Some(expected) => {
            crate::ssa_lower_call_arg_conv::emit_arg_conv(ctx, expected, value, v, owned)
        }
        None => v,
    };
    acc::class_accessor_call_args(ctx, fn_name, obj_val, &[(arg, Some(value))])
}

pub(crate) fn lower_struct_field_store(
    ctx: &mut LowerCtx<'_>,
    (assign, obj): (ExprId, ExprId),
    obj_val: Operand,
    sid: StructId,
    field: &str,
    value: ExprId,
) -> Operand {
    let layout = ctx.struct_layouts[sid.0 as usize].clone();
    let (idx, field_ty) = layout
        .iter()
        .enumerate()
        .find_map(|(i, (fname, fty))| {
            if fname == field {
                Some((i, *fty))
            } else {
                None
            }
        })
        .unwrap_or_else(|| panic!("ssa-lower: struct {sid:?} has no field `{field}`"));
    let offset = OBJ_HEADER_SIZE + (idx as u64) * 8;
    // T-09.d (v0.4.0) — frozen mutation guard. Inline call to runtime
    // helper that panics with a TypeError-shaped message if the
    // object's universal heap header has the FROZEN bit set. Matches
    // bun's strict-mode throw on `Object.freeze(o); o.field = ...`.
    // ~3-cycle overhead on the unfrozen path (single load + and + cmp
    // + branch-not-taken after LLVM inlines the call body).
    //
    // RFC 20260806-declared-field-redefine widened the question the
    // guard asks: a field demoted to `writable: false` by
    // defineProperty must refuse the store too, and its record lives
    // in the instance's sidecar. The field NAME therefore rides along
    // as a static literal (no rc traffic), and the header test that
    // was already happening covers both bits at once — an instance
    // that is neither frozen nor redefined pays exactly what it did.
    // It runs after the right-hand side (below): §10.1.9.2 [[Set]] is
    // inside PutValue, which §13.15.2 reaches only once the value exists.
    // V3-06 — `this.kids = []` in a constructor. Mirrors the K.6
    // LetDecl-global path: empty array literals lack inferable
    // element type on their own, so we allocate from the field's
    // declared `Type::Arr` here. An Any-elem field takes the
    // `arr_alloc_any` variant — the plain alloc left FLAG_ARR_ANY
    // clear, so every runtime flag-dispatch consumer (drop walker,
    // cycle collector, any-boxed readers) treated the array as
    // typed (RFC 20260706 Phase C conviction: an any[] field cycle
    // was invisible to the collector, and its NaN-box cell elems
    // were never dec'd on drop).
    let v = match store_rhs(ctx, (assign, obj), obj_val, sid, field, field_ty, value) {
        Rhs::Stored(v) => return v,
        Rhs::Value(v) => v,
    };
    // §13.15.2 — the value exists before PutValue asks anything of the
    // base. An absent receiver (§7.1.20 ToObject) and a frozen or
    // redefined field (§10.1.9.2) both refuse here, after the rhs; the
    // value is parked as a throw temp so either refusal releases the
    // stake it took above. The nullish test is self-gating: a receiver
    // that cannot be absent emits nothing.
    // A ctor's leading writes reach an instance nothing has seen yet
    // (`ssa_lower_fn::fresh_this`): no guard, and the first write to a
    // nullish-default field has nothing to release.
    let v_ty = ctx.operand_ty(&v);
    let parked = (!v_ty.is_copy()).then(|| ctx.push_throw_temp(v, v_ty));
    crate::ssa_lower_nullable_guard_emit::emit_undefable_heap_guard(ctx, obj, &obj_val);
    let done = guard::emit_field_writable_guard(ctx, obj, obj_val, (field, field_ty), v);
    if let Some(token) = parked {
        ctx.pop_throw_temp(token);
    }
    // Drop the old field value if non-Copy.
    if !field_ty.is_copy() && !ctx.proofs.fresh_this_first_write.contains(&obj) {
        let cur_block = ctx.cur_block;
        let old = ctx.f.append_inst(
            cur_block,
            InstKind::Load(field_ty, obj_val, offset),
            field_ty,
            None,
        );
        ctx.emit_drop_value(Operand::Value(old), field_ty);
    }
    // L3b #6 crash fix — mirror the ObjectLit field-store mark: a
    // typed Array entering a struct field must carry its elem kind
    // for the runtime walkers (inspect / cycle collector). No-op for
    // non-Arr fields. Chunk 621 — the chain now derives from the
    // value's own type inside the helper: an `any[]` field taking a
    // typed array (T-11 widen) marked chain 0 off the field type and
    // left the block invisible to the kind-aware readers.
    ctx.emit_arr_mark_kind(&v);
    let cur_block = ctx.cur_block;
    ctx.f
        .append_void(cur_block, InstKind::Store(v, obj_val, offset));
    guard::mint_read_stake(ctx, (assign, obj, field), v, done);
    guard::finish_guarded_store(ctx, done);
    v
}
