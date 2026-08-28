//! The typed-receiver tail of the member-assign ladder — the P8.2
//! accessor-setter direct call and the direct struct field store —
//! moved out of [`crate::ssa_lower_assign_member`] when the rotation
//! 240 setter-argument fix pushed that file past the 500-line limit
//! (the rotation 230 watch called this exact split).

use crate::ast::{Expr, ExprId};
use crate::ssa::{IPred, InstKind, Operand, StructId, Terminator, Type};
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
    // P8.2 — accessor write: `c.value = v` where C declares
    // `set value(n: T)`. desugar_classes renamed the setter's FnDecl
    // to `__cm_<C>__<name>_set` and recorded
    // `(C, name) → fn_name` in `ast.accessor_setters`. Emit a Call
    // to the setter with `[obj_val, value]` and return the value
    // (parallel to a normal Store which also evaluates to the
    // value). Skips the struct field lookup + Store path below.
    // RFC 20260715-nominal-class-identity — the setter's class comes
    // from the receiver's NAME, not from whichever class shares its
    // layout id.
    // Blade 2 (rotation 413) — the pair may live on an ANCESTOR;
    // walk the chain the way [[Set]] would, starting from the
    // receiver's FULL ClassRef key. A generic declarer's setter has
    // no fn_table entry under its bare name — the checker recorded
    // this assign site in generic_call_sites, so the mono retarget
    // names the specialization (blade 3/4).
    let cls_key = match ctx.expr_types.get(&obj)? {
        crate::check::Type::ClassRef(n)
            if ctx
                .ast
                .class_parents
                .contains_key(n.split('<').next().unwrap_or(n.as_str())) =>
        {
            n.clone()
        }
        _ => return None,
    };
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
    // field's +1 (struct-field share below), so a borrow-shape rhs
    // keeps the source binding's stake and an owned temp releases
    // its surplus after the call. Recorded edge (RFC 20260705
    // ledger): a non-storing setter + a consumer binding the assign
    // result reads a released temp — assign-result ownership is its
    // own lane.
    let transfers = ctx.expr_transfers_ownership(value);
    // F2-fix, generalized (rotation 240) — this is a direct Call, so
    // the argument crosses the same lane boundary as any direct
    // call's; the hand-written I64→F64 widen that sat here knew one
    // direction and handed every other mismatch verbatim. The killer
    // was the untyped setter (`set p(v)` — Any param): an i64 rhs
    // arrived as raw bits, the body stored them as a NaN-box, and the
    // bare-field slot held a garbage box (p25g SIGSEGV / silent
    // no-output). Route through the one `arg_conv` contract instead.
    let mut owned: Vec<(Operand, Type)> = Vec::new();
    let arg = match ctx.fn_sig_ids.get(&fid).copied() {
        // The `__cm_` setter's sig is receiver-first: param 0 is the
        // receiver, param 1 the user value.
        Some(sig_id) => match ctx.fn_sigs[sig_id.0 as usize].0.get(1).copied() {
            Some(expected) => crate::ssa_lower_call_arg_conv::emit_arg_conv(
                ctx,
                expected,
                value,
                v.clone(),
                &mut owned,
            ),
            None => v.clone(),
        },
        None => v.clone(),
    };
    let cur_block = ctx.cur_block;
    ctx.f
        .append_void(cur_block, InstKind::Call(fid, vec![obj_val, arg]));
    ctx.emit_throw_check(Some(fid));
    for (op, ty) in owned {
        ctx.emit_drop_value(op, ty);
    }
    let v_ty = ctx.operand_ty(&v);
    if transfers && v_ty.is_refcounted() {
        ctx.emit_drop_value(v.clone(), v_ty);
    }
    Some(v)
}

pub(crate) fn lower_struct_field_store(
    ctx: &mut LowerCtx<'_>,
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
    emit_field_writable_guard(ctx, obj_val.clone(), field);
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
        // Chunk 566 — a field store SHARES the rhs (TS has no move
        // semantics): a borrow-shape value takes +1 so the field
        // owns its stake while the source binding keeps its own —
        // the old consume let a re-assign's drop-old steal the
        // source's only ref (UAF, reuse-window probe-proven). Owned
        // temps keep transferring their fresh reference.
        let mut transfers = ctx.expr_transfers_ownership(value);
        // W4 — align the stored value with the field width (mirrors
        // the index-assign site; the reverse direction means the
        // width analysis missed this write).
        let v = match (field_ty, ctx.operand_ty(&v)) {
            (Type::F64, Type::I64) => ctx.coerce_to_f64(v),
            (Type::I64, Type::F64) => panic!(
                "ssa-lower: f64 value into i64 struct field `{field}` — \
                 container width analysis missed this write"
            ),
            // RFC 20260710 C4 — a declared-Any slot (`__nullable(
            // number|boolean)` optional field, plain `any` field)
            // takes a NaN-box: box the scalar / nullish-literal
            // write (expr-aware — an undefined literal boxes to
            // ANY_UNDEF, a null literal to ANY_NULL). The box is an
            // rc-inert immediate for these payloads, so the share
            // inc below no-ops through __torajs_rc_inc's NaN-box
            // gate. Heap sources keep their pre-RFC raw store —
            // their ownership story is a separate face.
            (Type::Any, Type::I64 | Type::I32 | Type::F64 | Type::Bool) => {
                ctx.box_to_any_from_expr(value, v)
            }
            (Type::Any, Type::Ptr) if matches!(v, Operand::ConstPtrNull) => {
                ctx.box_to_any_from_expr(value, v)
            }
            // S2.26 (RFC 20260727-dstr-assignment 刀 4) — the reverse
            // direction: an Any value into a declared scalar / Str
            // field unboxes through the same kernels every other
            // any→typed sink uses (coerce_for_local / coerce_for_
            // global). The fall-through used to store the NaN-box
            // bits raw — `o.k = (v: any) 9` read back as NaN.
            // Bool / heap-typed fields still fall through (no
            // established unbox kernel carries their guard story) —
            // the registered S2.26 remainder.
            (Type::F64 | Type::I64, Type::Any) => ctx.coerce_any_to_number(v, field_ty),
            (Type::Str, Type::Any) => {
                // This coercion MINTS: `any_to_str` hands back a
                // reference of its own (an rc_inc on a plain Str, a
                // fresh allocation for everything else). What reaches
                // the slot is therefore not the source expression's
                // value any more, so the ownership question asked
                // above — about the SOURCE — is about the wrong
                // thing. A borrowed `any` ident answered "does not
                // transfer", the store retained a second time, and
                // `new Error("x")` stranded one Str per call: 33
                // bytes per construction, unbounded. The scalar arms
                // above need no such correction (their results are
                // Copy) and the boxing arms mint rc-inert immediates.
                transfers = true;
                ctx.coerce_to_str(v, Type::Any)
            }
            _ => v,
        };
        let v_ty = ctx.operand_ty(&v);
        if !transfers && !v_ty.is_copy() {
            ctx.emit_rc_inc(v.clone());
        }
        v
    };
    // Drop the old field value if non-Copy.
    if !field_ty.is_copy() {
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
    v
}

/// Frozen / redefined-field guard before a struct field store.
///
/// Rotation 470 — the two bits the helper tests are read INLINE (the
/// u16 `flags` at +6 is the high half of the u32 at +4, whose low half
/// is `type_tag`); `__torajs_obj_check_field_writable` and its throw
/// check only run on a hit. An unfrozen, never-redefined instance —
/// every instance in a hot ctor loop — pays one load, one and, one
/// not-taken branch instead of two cross-archive calls (~6% + ~3% of
/// `class-method`). Leaves `cur_block` on the store block.
fn emit_field_writable_guard(ctx: &mut LowerCtx, obj_val: Operand, field: &str) {
    let hdr_word = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I32, obj_val.clone(), 4),
        Type::I32,
        None,
    );
    let guard_bits = ((torajs_rc::FLAG_FROZEN | torajs_rc::FLAG_OBJ_EXOTIC_FIELD) as u32) << 16;
    let masked = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::BinOp(
            crate::ssa::BinOp::And,
            Operand::Value(hdr_word),
            Operand::ConstI32(guard_bits as i32),
        ),
        Type::I32,
        None,
    );
    let hit = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Ne, Operand::Value(masked), Operand::ConstI32(0)),
        Type::Bool,
        None,
    );
    let check_blk = ctx.f.add_block();
    let store_blk = ctx.f.add_block();
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(hit),
            then_blk: check_blk,
            else_blk: store_blk,
        },
    );
    ctx.cur_block = check_blk;
    let name_str = ctx.intern_string_literal(field);
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.obj_check_field_writable,
            vec![obj_val.clone(), Operand::Value(name_str)],
        ),
    );
    // P7.4-frozen — obj_check_not_frozen now arms a real TypeError
    // (instead of process abort) when the target is frozen. Force
    // the throw-check here (intrinsic → emit_throw_check(Some) would
    // skip it) so it diverts to the try/catch or propagates BEFORE
    // the field store below — the illegal mutation must not happen.
    // Mirrors the a-2 dynobj writable=false pattern.
    ctx.emit_throw_check(None);
    let cb = ctx.cur_block;
    ctx.f.set_term(cb, Terminator::Br(store_blk));
    ctx.cur_block = store_blk;
}
