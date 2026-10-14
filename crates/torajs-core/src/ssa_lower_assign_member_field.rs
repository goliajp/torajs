//! The typed-receiver tail of the member-assign ladder — the P8.2
//! accessor-setter direct call and the direct struct field store —
//! moved out of [`crate::ssa_lower_assign_member`] when the rotation
//! 240 setter-argument fix pushed that file past the 500-line limit
//! (the rotation 230 watch called this exact split).

mod fit;
pub(crate) mod guard;

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, StructId, Type};
use crate::ssa_lower::{LowerCtx, OBJ_HEADER_SIZE};

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
    let done = guard::emit_field_writable_guard(ctx, obj, obj_val, field, v);
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

/// What the rhs of a struct field store became.
enum Rhs {
    /// A value for the slot, owned by the field once stored.
    Value(Operand),
    /// The any lane stored it already; this is the assignment's value.
    Stored(Operand),
}

/// Lower the rhs `value` of the store into `obj.field` (slot type
/// `field_ty`) and bring it into the slot's shape — or, for a value a
/// number slot cannot spell, store it through the any lane.
fn store_rhs(
    ctx: &mut LowerCtx<'_>,
    (assign, obj): (ExprId, ExprId),
    obj_val: Operand,
    sid: StructId,
    field: &str,
    field_ty: Type,
    value: ExprId,
) -> Rhs {
    let v = if matches!(
        ctx.expr_types.get(&value),
        Some(crate::check::Type::Undefined)
    ) && let Some(sentinel) = ctx.str_undef_sentinel_for(field_ty)
    {
        // RFC 20260710-optional-undefined-repr C1 — an undefined
        // LITERAL into a Str/Substr slot stores the per-type
        // sentinel cell, not NULL (which means JS null). The cell
        // is immortal (FLAG_STATIC_LITERAL — rc/drop no-ops), so
        // no inc; the drop-old below stays correct for whatever
        // the slot held.
        sentinel
    } else if let Expr::Array(els) = ctx.ast.get_expr(value)
        && els.is_empty()
        && matches!(field_ty, Type::Arr(_))
    {
        let alloc_fn = if let Type::Arr(arr_id) = field_ty
            && ctx.arr_layouts[arr_id.0 as usize] == Type::Any
        {
            ctx.intrinsics.arr_alloc_any
        } else {
            ctx.intrinsics.arr_alloc
        };
        let cur_block = ctx.cur_block;
        let alloc = ctx.f.append_inst(
            cur_block,
            InstKind::Call(alloc_fn, vec![Operand::ConstI64(0)]),
            field_ty,
            None,
        );
        Operand::Value(alloc)
    } else if field_ty == Type::Any
        && let Some(w) =
            crate::ssa_lower_dstr_iter::try_lower_field_walk(ctx, value, &obj_val, sid, field)
    {
        // Rotation 455 — a generator-lifted destructure group temp
        // (`this.__dstra_src_N = init`, checker-recorded in
        // `iter_destr_srcs`): step the source through the iterator
        // protocol instead of storing it raw, so the index reads
        // below the lift land on a real Array<Any>. The walk's boxed
        // result is an OWNED stake the field takes verbatim — no
        // share inc (the general arm's `transfers` question is about
        // the SOURCE expression, which the walk already settled).
        w
    } else if field_ty == Type::Any && matches!(ctx.ast.get_expr(value), Expr::ObjectLit { .. }) {
        // Rotation 646 — an ObjectLit into a declared-Any field takes
        // the P3.2 dynobj lane the LetDecl path has
        // (`lower_let_init_val`). The plain lower minted a STRUCT and
        // the raw store boxed it, so `k.p = {}` on a class field — and
        // every `const p: any = {}` a generator lifts into its env —
        // carried a struct where `let p: any = {}` carries a dynobj:
        // `Object.defineProperty(p, 0, { get })` then `p[0]` read the
        // struct's expando lane, and the two spellings of one program
        // disagreed. The minted cell is an owned stake the field takes
        // verbatim (the any-literal array arm's shape).
        ctx.lower_dynobj_init(value)
    } else if let Expr::Array(els) = ctx.ast.get_expr(value)
        && let Type::Arr(arr_id) = field_ty
        && ctx.arr_layouts[arr_id.0 as usize] == Type::Any
    {
        // Chunk 614 — non-empty literal into an Any-elem field takes
        // the same annotation-consuming widen the LetDecl path has
        // (`lower_let_init_val`): without it the literal lowered
        // through the typed fast path, so the stored block never got
        // FLAG_ARR_ANY — the cycle collector's `is_visitable_arr`
        // said leaf, an `any[]` field cycle was invisible (obj root
        // buffered but the trial-deletion walk never crossed the
        // arr), and its raw scalar slots were NaN-box-misread by
        // every flag-dispatch consumer.
        let ids: Vec<ExprId> = els.clone();
        ctx.lower_array_any_literal(&ids)
    } else {
        // Chunk 784 — pin the field's declared struct layout for a
        // direct ObjectLit rhs (mirrors the chunk-780 let-decl site):
        // without it resolve_objlit_layout first-matches a
        // same-shaped layout registered under a different declared
        // type and the slot reprs collide (silent-wrong reads
        // through the declared layout).
        if let Type::Obj(inner_sid) = field_ty
            && matches!(ctx.ast.get_expr(value), Expr::ObjectLit { .. })
        {
            ctx.let_declared_obj_layout = Some(inner_sid);
        }
        let v = ctx.lower_expr(value);
        ctx.let_declared_obj_layout = None;
        // a value a number slot cannot spell (an any-lane value, a
        // `null`) takes the any lane's [[Set]]: a number lands in the
        // slot, anything else in the field's sidecar
        let cannot_spell = crate::ssa_lower_object_lit::absent::number_slot_cannot_spell;
        if cannot_spell(ctx, field_ty, v, value) {
            let boxed = ctx.box_to_any(obj_val);
            let stored = crate::ssa_lower_assign_member_any::lower_dynobj_assign_value(
                ctx,
                assign,
                boxed,
                field,
                (value, v),
                &None,
                false,
            );
            return Rhs::Stored(stored);
        }
        // Chunk 566 — a field store SHARES the rhs (TS has no move
        // semantics): a borrow-shape value takes +1 so the field
        // owns its stake while the source binding keeps its own —
        // the old consume let a re-assign's drop-old steal the
        // source's only ref (UAF, reuse-window probe-proven). Owned
        // temps keep transferring their fresh reference — and so
        // does a fit that MINTED, whose value the source expression
        // never held (see `fit_to_field`).
        let (v, minted) = fit::fit_to_field(ctx, field_ty, v, value, (obj, field));
        let transfers = ctx.expr_transfers_ownership(value) || minted;
        let v_ty = ctx.operand_ty(&v);
        if !transfers && !v_ty.is_copy() {
            ctx.emit_rc_inc(v);
        }
        v
    };
    Rhs::Value(v)
}
