//! M1.2 — `Expr::Array(elements)` lowering pulled out of
//! [`crate::ssa_lower::lower_expr_inner`]'s match arm as chunk-76
//! of the decomp (chunks 1-75 = ... + `Expr::ObjectLit` 5-phase).
//! **MAIN PRIZE** of the god-arm decomp: the largest Expr variant
//! by inline LOC.
//!
//! Five dispatch layers tried in source order:
//!
//! 1. **Empty `[]`** — P0.10: bare `[]` in non-let-init expression
//!    position defaults to `Array<Any>` (mirrors check.rs Expr
//!    ::Array empty default). Routes through `arr_alloc_any` for
//!    the 16-byte tagged-slot stride.
//! 2. **T-10.c heterogeneous fast path** — `[1, 'a', true]` routes
//!    through `lower_array_any_literal` (check.rs already widened
//!    the slot type to Array<Any>; we emit the matching codegen).
//!    Cheap AST-shape probe: if element kinds differ AND no
//!    spread, delegate.
//! 3. **No-spread typed path** — `alloc(cap=N)`, set `len=N`,
//!    direct stores at `ARR_DATA_OFF + i*8`. 11-A2-a stack-alloca
//!    path when escape verifier flagged the literal AND elements
//!    are non-refcounted (refcounted forces heap because
//!    STATIC_LITERAL flag short-circuits arr_drop's element walk,
//!    leaking rc refs). Anchor type from first non-empty sibling;
//!    empty `[]` inners get typed `arr_alloc(0)`. W4 alias-class
//!    width widen (`[1, 0.5]` + later `a[0] = 0.5` → F64 elements).
//! 4. **Spread path** — pre-compute total length at runtime
//!    `(literal_count + sum spread.length)`, `alloc(cap=total)`
//!    with len=0, fill via per-element `arr_push_unchecked` +
//!    per-spread `arr_extend_unchecked`. Spread sources are
//!    memcpy'd in one shot — no per-element runtime call, single
//!    alloc, no realloc. Spread source normalization:
//!    - **S134**: string spread `[...str]` unfolds per code unit
//!      via `str_split("")` → Arr<Substr> → Arr<Str> materialize.
//!      Substr source first materializes to owned Str.
//!    - **Set spread**: `[...set]` reuses `arr_from_set::emit`
//!      to walk map-iter bucket chain into a fresh Arr<Any>.
//!      Element type from first non-spread literal OR first spread
//!      source's element type. Array<Any> spread re-routes through
//!      `lower_array_any_literal` (the 8-byte slot path here would
//!      mis-stride the 16-byte tagged layout).
//! 5. **Refcount discipline** — for refcounted element types,
//!    `emit_rc_inc` per literal AND per appended spread element
//!    (via `emit_arr_rc_inc_range` over `old_len..new_len`); leave
//!    source ident live so its scope-drop fires; the inc balances
//!    the array's element-walk dec. For Copy / non-refcounted-
//!    non-Copy keep legacy consume-if-ident transfer until Phase
//!    2 migrates those layouts.
//!
//! Returns `Operand` directly (terminal arm — caller's
//! `Expr::Array` match arm bottoms out here).

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx, intern_arr_layout};
use crate::ssa_lower_array_alloc::{alloc_heap_arr, alloc_stack_arr};
use crate::ssa_lower_intrinsics_str_b::STR_UNDEF_CELL_SYM;

pub(crate) fn lower(ctx: &mut LowerCtx<'_>, elements: &[ExprId], eid: ExprId) -> Operand {
    // claimed here, so no element's own literal takes it
    let declared = ctx.take_declared_arr();
    if elements.is_empty() {
        return lower_empty(ctx);
    }
    let element_ids: Vec<ExprId> = elements.to_vec();
    let has_spread = element_ids
        .iter()
        .any(|eid| matches!(ctx.ast.get_expr(*eid), Expr::Spread { .. }));
    if crate::ssa_lower_array_any_gate::takes_any_lane(ctx, &element_ids, eid, has_spread) {
        return ctx.lower_array_any_literal(&element_ids);
    }
    if !has_spread {
        return lower_no_spread(ctx, &element_ids, eid, declared.as_ref());
    }
    crate::ssa_lower_array_spread::lower_spread(ctx, &element_ids, eid)
}

fn lower_empty(ctx: &mut LowerCtx<'_>) -> Operand {
    let arr_id = intern_arr_layout(ctx.arr_layouts, Type::Any);
    let cur_block = ctx.cur_block;
    let alloc_call = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.arr_alloc_any, vec![Operand::ConstI64(0)]),
        Type::Arr(arr_id),
        None,
    );
    Operand::Value(alloc_call)
}

/// The typed no-spread literal, built in place: the block is allocated
/// as soon as the slot type is known (the anchor probe, or the first
/// element when nothing anchors) and every element is stored the
/// moment it is lowered, `len` advancing one slot at a time. The block
/// itself is the one parked temp — a throw between two elements drops
/// it, and its drop walks exactly the stored prefix, so an owned
/// element is released once whichever side of the store the throw
/// falls on (the rotation-549 shape, with the block standing in for
/// the per-element parks).
///
/// 555-01 — the old shape lowered EVERY element first and stored them
/// at the end, so all n values were live across the whole literal:
/// a 65k-pair Unicode table spilled every one of them (frame 524 KiB,
/// past the 32 KiB addressing cap) and the allocator's active set
/// grew with n. Storing as we go bounds the live set by the element
/// being built.
fn lower_no_spread(
    ctx: &mut LowerCtx<'_>,
    element_ids: &[ExprId],
    eid: ExprId,
    declared: Option<&(Type, Option<String>)>,
) -> Operand {
    let elem = declared.map(|(ty, ann)| declared_elem(ctx, *ty, ann.as_deref()));
    let elem = elem.as_ref();
    let (anchor_ty, mut probed) = probe_anchor_ty(ctx, element_ids, elem);
    // Nothing anchors (empty literals / nullish constants only): the
    // first element's own lowering decides the slot type, as
    // `compute_elem_ty` always read it off the first value. The
    // element lowering still sees NO anchor — an all-empty literal's
    // inner `[]`s keep their `lower_empty` (any-flavored) blocks, not
    // the anchored `arr_alloc(0)` mint, exactly as before
    // (`check-arr-nested-empty-literal-001`: a push through the
    // `any[]` view of an anchored mint changed the element kind).
    let slot_anchor = match anchor_ty {
        Some(t) => t,
        None => {
            let (v, _) = lower_no_spread_element(ctx, element_ids[0], None, elem);
            let t = ctx.operand_ty(&v);
            probed = Some((0, v));
            t
        }
    };
    let elem_ty = compute_elem_ty(ctx, slot_anchor, eid);
    let arr_id = intern_arr_layout(ctx.arr_layouts, elem_ty);
    let n = element_ids.len() as i64;
    let on_stack = ctx.ast.stack_array_literals.contains(&eid) && !elem_ty.is_refcounted();
    let arr_ptr = if on_stack {
        alloc_stack_arr(ctx, arr_id, n)
    } else {
        alloc_heap_arr(ctx, arr_id, n)
    };
    // A heap cell's length tracks the filled prefix, so a throw from a
    // later element releases exactly what was stored. A stack block is
    // never released on a throw and holds no counted elements: it
    // takes its length once, after the last element.
    if !on_stack {
        let cur_block = ctx.cur_block;
        ctx.f.append_void(
            cur_block,
            InstKind::Store(Operand::ConstI64(0), Operand::Value(arr_ptr), ARR_LEN_OFF),
        );
    }
    let data = ctx.emit_arr_data_ptr(Operand::Value(arr_ptr));
    // A stack block is not a heap cell — nothing to drop on a throw.
    let arr_tok =
        (!on_stack).then(|| ctx.push_throw_temp(Operand::Value(arr_ptr), Type::Arr(arr_id)));
    for (i, &elem_eid) in element_ids.iter().enumerate() {
        let (mut val, mut inc_after) = match &probed {
            Some((p_idx, p_op)) if *p_idx == i => {
                let op = *p_op;
                let inc = elem_needs_inc(ctx, elem_eid, &op);
                (op, inc)
            }
            _ => lower_no_spread_element(ctx, elem_eid, anchor_ty, elem),
        };
        if elem_ty == Type::F64 && ctx.operand_ty(&val) == Type::I64 {
            val = ctx.coerce_to_f64(val);
        }
        if elem_ty == Type::Str && ctx.operand_ty(&val) == Type::Substr {
            val = materialize_substr_elem(ctx, val, elem_eid);
            inc_after = false;
        }
        // RFC 20260707 chunk 2 — an `undefined` element in a
        // Str-typed array literal (`["a", undefined]`) stores the
        // undefined sentinel cell, not NULL, so eq/print/JSON agree
        // with the flipped exec/match miss-capture slots.
        if elem_ty == Type::Str
            && matches!(val, Operand::ConstPtrNull)
            && matches!(
                ctx.expr_types.get(&elem_eid),
                Some(crate::check::Type::Undefined)
            )
        {
            let cur_block = ctx.cur_block;
            let u = ctx.f.append_inst(
                cur_block,
                InstKind::GlobalRef(STR_UNDEF_CELL_SYM.to_string()),
                Type::Str,
                None,
            );
            val = Operand::Value(u);
        }
        let cur_block = ctx.cur_block;
        ctx.f
            .append_void(cur_block, InstKind::Store(val, data, (i as u64) * 8));
        if !on_stack {
            ctx.f.append_void(
                cur_block,
                InstKind::Store(
                    Operand::ConstI64(i as i64 + 1),
                    Operand::Value(arr_ptr),
                    ARR_LEN_OFF,
                ),
            );
        }
        if inc_after {
            ctx.emit_rc_inc(val);
        }
    }
    if let Some(t) = arr_tok {
        ctx.pop_throw_temp(t);
    }
    if on_stack {
        let cur_block = ctx.cur_block;
        ctx.f.append_void(
            cur_block,
            InstKind::Store(Operand::ConstI64(n), Operand::Value(arr_ptr), ARR_LEN_OFF),
        );
    }
    Operand::Value(arr_ptr)
}

/// Lower the first non-empty-array element to learn the anchor type.
/// The lowered operand is returned alongside (keyed by its element
/// index) so the collect loop reuses it instead of lowering the
/// expression again — a second lower re-emits the element's SSA
/// (double side-effect: `[step(), ..]` called step twice) and leaked
/// the first evaluation's owned result (RFC 20260705 chunk 547).
fn probe_anchor_ty(
    ctx: &mut LowerCtx<'_>,
    element_ids: &[ExprId],
    elem: Option<&(Type, Option<String>)>,
) -> (Option<Type>, Option<(usize, Operand)>) {
    for (idx, eid) in element_ids.iter().enumerate() {
        if matches!(ctx.ast.get_expr(*eid), Expr::Array(els) if els.is_empty()) {
            continue;
        }
        // RFC 20260714-dstr-residual — a syntactic nullish constant
        // (`undefined` ident / `null` literal, incl. elision holes)
        // must not anchor the literal: `[undefined, "z"]` anchored
        // Ptr and stored the Str sibling raw (read answered the
        // pointer as a number — silent-wrong). Skipping realizes
        // chunk 807's documented invariant (the Str sentinel lane
        // assumed a Str anchor, which only held with the string
        // first). Shape-matched, not type-matched: a void-call
        // element stays probe-eligible so evaluation order holds.
        match ctx.ast.get_expr(*eid) {
            Expr::Ident(n) if n == "undefined" => continue,
            Expr::Null => continue,
            _ => {}
        }
        let probe = lower_pinned(ctx, *eid, elem);
        let ty = ctx.operand_ty(&probe);
        return (Some(ty), Some((idx, probe)));
    }
    (None, None)
}

/// One element of the typed no-spread literal: the value and whether
/// the slot owes it a share (`inc_after`). An empty nested literal
/// under an Arr anchor mints its block at the anchor's layout.
fn lower_no_spread_element(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    anchor_ty: Option<Type>,
    elem: Option<&(Type, Option<String>)>,
) -> (Operand, bool) {
    if matches!(ctx.ast.get_expr(eid), Expr::Array(els) if els.is_empty())
        && let Some(Type::Arr(inner_id)) = anchor_ty
    {
        let cur_block = ctx.cur_block;
        let v = ctx.f.append_inst(
            cur_block,
            InstKind::Call(ctx.intrinsics.arr_alloc, vec![Operand::ConstI64(0)]),
            Type::Arr(inner_id),
            None,
        );
        return (Operand::Value(v), false);
    }
    let v = lower_pinned(ctx, eid, elem);
    let inc = elem_needs_inc(ctx, eid, &v);
    (v, inc)
}

/// The element type of the declared array type `ty` annotated `ann`,
/// and the element's annotation.
fn declared_elem(ctx: &LowerCtx<'_>, ty: Type, ann: Option<&str>) -> (Type, Option<String>) {
    let Type::Arr(id) = ty else {
        unreachable!("an array literal is pinned only to an array type")
    };
    let elem_ann = ann.and_then(crate::ast_throw_info::elem_ann);
    (ctx.arr_layouts[id.0 as usize], elem_ann)
}

/// The element `eid` lowered, pinned to the declared element type
/// `elem` when it is a literal of that type: the readers of the array
/// read every element at the declared element layout.
fn lower_pinned(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    elem: Option<&(Type, Option<String>)>,
) -> Operand {
    let Some((ty, ann)) = elem else {
        return ctx.lower_expr(eid);
    };
    ctx.pin_declared(*ty, ann.as_deref(), eid);
    let v = ctx.lower_expr(eid);
    ctx.let_declared_obj_layout = None;
    v
}

/// Chunk 570 — a named-binding elem is always a SHARE: local OR
/// global (the old `locals`-only lookup answered false for a
/// top-level source, so the slot stole the global's only ref and the
/// array's death freed it — UAF, probe-proven; 564 apply_borrow_rc_inc
/// mirror), moved bindings included (their cell is alive under the
/// canonical owner). Peel value-transparent `As` wrappers and a
/// comma's discarded left operand first, for
/// the reason the objlit field / assign-target / return siblings all
/// peel: `lower_as_cast` answers the inner operand untouched for a
/// heap source, so the inner read decides whether the slot owes a
/// share. Unpeeled, `[src as string]` stored the binding's pointer
/// bare and the array outlived the source's scope drop.
fn elem_needs_inc(ctx: &LowerCtx<'_>, eid: ExprId, v: &Operand) -> bool {
    if !ctx.operand_ty(v).is_refcounted() {
        return false;
    }
    let src_eid = crate::ssa_lower_return_borrow::peel_passthrough(ctx.ast, eid);
    match ctx.ast.get_expr(src_eid) {
        Expr::Ident(name) => ctx.locals.contains_key(name) || ctx.globals.contains_key(name),
        Expr::Member { .. } | Expr::Index { .. } => true,
        // Hoisted regex-literal singleton (fn-scope LICM) — the slot
        // takes a share; see apply_borrow_rc_inc.
        Expr::Regex { .. } => true,
        _ => false,
    }
}

fn compute_elem_ty(ctx: &LowerCtx<'_>, anchor_ty: Type, eid: ExprId) -> Type {
    let mut elem_ty = anchor_ty;
    if elem_ty == Type::I64
        && ctx
            .num_f64_slots
            .elem_is_f64(&crate::num_width::SlotKey::Anon(eid.0))
    {
        elem_ty = Type::F64;
    }
    // A Substr anchor (`[s[1], ...]`) never mints an Arr<Substr> —
    // array slots hold owned Str; the element loop materializes
    // every view (`materialize_substr_elem`).
    if elem_ty == Type::Substr {
        elem_ty = Type::Str;
    }
    elem_ty
}

/// A Substr element (`["z", s[1]]`) must not store its view pointer
/// into a Str slot — the two block layouts diverge past the header,
/// so every downstream Str-layout read (join / print / sort) walks
/// garbage. Materialize the view to an owned Str (`substr_to_owned`;
/// the undefined sentinel propagates identity). The owned block
/// transfers into the slot, so the caller flips `inc_after` off;
/// fresh-view producers (index mint / method call) hand us the view's
/// only ref — drop it — while borrowed views (ident / member reads
/// off a live binding) stay with their owner.
fn materialize_substr_elem(ctx: &mut LowerCtx<'_>, view: Operand, eid: ExprId) -> Operand {
    let cur_block = ctx.cur_block;
    let owned = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.substr_to_owned, vec![view]),
        Type::Str,
        None,
    );
    let src = crate::ssa_lower_return_borrow::peel_passthrough(ctx.ast, eid);
    let borrowed = matches!(ctx.ast.get_expr(src), Expr::Ident(_) | Expr::Member { .. });
    if !borrowed {
        ctx.emit_drop_value(view, Type::Substr);
    }
    Operand::Value(owned)
}
