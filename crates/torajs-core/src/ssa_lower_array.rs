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
//!    Element type from first non-spread literal OR first spread
//!    source's element type. Array<Any> spread re-routes through
//!    `lower_array_any_literal` (the 8-byte slot path here would
//!    mis-stride the 16-byte tagged layout).
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
use crate::ssa::{BinOp as SsaBinOp, InstKind, Operand, Type};
use crate::ssa_lower::{ARR_DATA_OFF, ARR_LEN_OFF, ARR_PROPS_OFF, LowerCtx, intern_arr_layout};

#[derive(Debug)]
enum Item {
    Lit(Operand),
    Spread(Operand),
}

pub(crate) struct LoweredItem {
    pub(crate) op: Operand,
    pub(crate) src_eid: ExprId,
    pub(crate) is_spread: bool,
    /// Spread source lowered to `Type::Any` and was materialized into
    /// an owned `Arr<Any>` temp (RFC 20260704 S5+) — the assembler
    /// (`ssa_lower_arr_from_any::assemble_any_spread`) drops it after
    /// the extend.
    pub(crate) was_any: bool,
}

pub(crate) fn lower(ctx: &mut LowerCtx<'_>, elements: &[ExprId], eid: ExprId) -> Operand {
    if elements.is_empty() {
        return lower_empty(ctx);
    }
    let element_ids: Vec<ExprId> = elements.to_vec();
    let has_spread = element_ids
        .iter()
        .any(|eid| matches!(ctx.ast.get_expr(*eid), Expr::Spread { .. }));
    if !has_spread && ctx.array_literal_is_heterogeneous(&element_ids) {
        return ctx.lower_array_any_literal(&element_ids);
    }
    if !has_spread {
        return lower_no_spread(ctx, &element_ids, eid);
    }
    lower_spread(ctx, &element_ids, eid)
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

fn lower_no_spread(ctx: &mut LowerCtx<'_>, element_ids: &[ExprId], eid: ExprId) -> Operand {
    let n = element_ids.len() as i64;
    let (anchor_ty, probed) = probe_anchor_ty(ctx, element_ids);
    let (mut elem_vals, elem_inc_after) =
        lower_no_spread_elements(ctx, element_ids, anchor_ty, probed);
    let elem_ty = compute_elem_ty(ctx, anchor_ty, &elem_vals, eid);
    if elem_ty == Type::F64 {
        coerce_elem_vals_to_f64(ctx, &mut elem_vals);
    }
    let arr_id = intern_arr_layout(ctx.arr_layouts, elem_ty);
    let on_stack = ctx.ast.stack_array_literals.contains(&eid) && !elem_ty.is_refcounted();
    let arr_ptr = if on_stack {
        alloc_stack_arr(ctx, arr_id, n)
    } else {
        alloc_heap_arr(ctx, arr_id, n)
    };
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Store(Operand::ConstI64(n), Operand::Value(arr_ptr), ARR_LEN_OFF),
    );
    for (i, val) in elem_vals.iter().enumerate() {
        let off = ARR_DATA_OFF + (i as u64) * 8;
        let cur_block = ctx.cur_block;
        ctx.f.append_void(
            cur_block,
            InstKind::Store(*val, Operand::Value(arr_ptr), off),
        );
        if elem_inc_after[i] {
            ctx.emit_rc_inc(*val);
        }
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
) -> (Option<Type>, Option<(usize, Operand)>) {
    for (idx, eid) in element_ids.iter().enumerate() {
        if matches!(ctx.ast.get_expr(*eid), Expr::Array(els) if els.is_empty()) {
            continue;
        }
        let probe = ctx.lower_expr(*eid);
        let ty = ctx.operand_ty(&probe);
        return (Some(ty), Some((idx, probe)));
    }
    (None, None)
}

fn lower_no_spread_elements(
    ctx: &mut LowerCtx<'_>,
    element_ids: &[ExprId],
    anchor_ty: Option<Type>,
    probed: Option<(usize, Operand)>,
) -> (Vec<Operand>, Vec<bool>) {
    let mut elem_vals: Vec<Operand> = Vec::with_capacity(element_ids.len());
    let mut elem_inc_after: Vec<bool> = Vec::with_capacity(element_ids.len());
    for (idx, eid) in element_ids.iter().enumerate() {
        if matches!(ctx.ast.get_expr(*eid), Expr::Array(els) if els.is_empty())
            && let Some(Type::Arr(inner_id)) = anchor_ty
        {
            let cur_block = ctx.cur_block;
            let v = ctx.f.append_inst(
                cur_block,
                InstKind::Call(ctx.intrinsics.arr_alloc, vec![Operand::ConstI64(0)]),
                Type::Arr(inner_id),
                None,
            );
            elem_vals.push(Operand::Value(v));
            elem_inc_after.push(false);
            continue;
        }
        let v = match &probed {
            Some((p_idx, p_op)) if *p_idx == idx => p_op.clone(),
            _ => ctx.lower_expr(*eid),
        };
        let v_ty = ctx.operand_ty(&v);
        // Chunk 570 — a named-binding elem is always a SHARE: local
        // OR global (the old `locals`-only lookup answered false for
        // a top-level source, so the slot stole the global's only
        // ref and the array's death freed it — UAF, probe-proven;
        // 564 apply_borrow_rc_inc mirror), moved bindings included
        // (their cell is alive under the canonical owner).
        let needs_inc = v_ty.is_refcounted()
            && match ctx.ast.get_expr(*eid) {
                Expr::Ident(name) => {
                    ctx.locals.contains_key(name) || ctx.globals.contains_key(name)
                }
                Expr::Member { .. } | Expr::Index { .. } => true,
                _ => false,
            };
        elem_inc_after.push(needs_inc);
        elem_vals.push(v);
    }
    (elem_vals, elem_inc_after)
}

fn compute_elem_ty(
    ctx: &LowerCtx<'_>,
    anchor_ty: Option<Type>,
    elem_vals: &[Operand],
    eid: ExprId,
) -> Type {
    let mut elem_ty = anchor_ty.unwrap_or_else(|| ctx.operand_ty(&elem_vals[0]));
    if elem_ty == Type::I64
        && ctx
            .num_f64_slots
            .elem_is_f64(&crate::num_width::SlotKey::Anon(eid.0))
    {
        elem_ty = Type::F64;
    }
    elem_ty
}

fn coerce_elem_vals_to_f64(ctx: &mut LowerCtx<'_>, elem_vals: &mut [Operand]) {
    for v in elem_vals.iter_mut() {
        if ctx.operand_ty(v) == Type::I64 {
            *v = ctx.coerce_to_f64(v.clone());
        }
    }
}

fn alloc_stack_arr(
    ctx: &mut LowerCtx<'_>,
    arr_id: crate::ssa::ArrId,
    n: i64,
) -> crate::ssa::ValueId {
    let total_bytes = ARR_DATA_OFF + (n as u64) * 8;
    let cur_block = ctx.cur_block;
    let p = ctx.f.append_inst(
        cur_block,
        InstKind::AllocaBytes(total_bytes),
        Type::Arr(arr_id),
        None,
    );
    // Header packed: tag=2 (ARR) bits 32..48, flags=4 (STATIC) bits 48..64.
    let hdr_packed: i64 = (2i64 << 32) | (4i64 << 48);
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Store(Operand::ConstI64(hdr_packed), Operand::Value(p), 0),
    );
    ctx.f.append_void(
        cur_block,
        InstKind::Store(Operand::ConstI64(n), Operand::Value(p), 16),
    );
    ctx.f.append_void(
        cur_block,
        InstKind::Store(Operand::ConstI64(0), Operand::Value(p), ARR_PROPS_OFF),
    );
    p
}

fn alloc_heap_arr(
    ctx: &mut LowerCtx<'_>,
    arr_id: crate::ssa::ArrId,
    n: i64,
) -> crate::ssa::ValueId {
    let cur_block = ctx.cur_block;
    ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.arr_alloc, vec![Operand::ConstI64(n)]),
        Type::Arr(arr_id),
        None,
    )
}

fn lower_spread(ctx: &mut LowerCtx<'_>, element_ids: &[ExprId], eid: ExprId) -> Operand {
    let (lowered, elem_ty, literal_count) = lower_spread_elements(ctx, element_ids);
    // Any element type assembles from the ALREADY-lowered operands —
    // re-lowering the ExprIds (the pre-S5+ shape) would double-emit
    // spread source side effects (e.g. `[...m.values()]` minting the
    // iterator twice).
    if matches!(elem_ty, Some(Type::Any)) {
        return crate::ssa_lower_arr_from_any::assemble_any_spread(ctx, lowered, literal_count);
    }
    let elem_is_refcounted = elem_ty.unwrap_or(Type::I64).is_refcounted();
    let items = build_items(ctx, &lowered, elem_is_refcounted);
    let mut elem_ty = elem_ty.unwrap_or(Type::I64);
    if elem_ty == Type::I64
        && ctx
            .num_f64_slots
            .elem_is_f64(&crate::num_width::SlotKey::Anon(eid.0))
    {
        elem_ty = Type::F64;
    }
    let arr_id = intern_arr_layout(ctx.arr_layouts, elem_ty);
    let elem_is_refcounted = elem_ty.is_refcounted();
    let total = compute_total_length(ctx, &items, literal_count);
    let cur_block = ctx.cur_block;
    let arr_ptr = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.arr_alloc, vec![total]),
        Type::Arr(arr_id),
        None,
    );
    fill_arr_from_items(ctx, arr_ptr, items, elem_ty, elem_is_refcounted);
    Operand::Value(arr_ptr)
}

fn lower_spread_elements(
    ctx: &mut LowerCtx<'_>,
    element_ids: &[ExprId],
) -> (Vec<LoweredItem>, Option<Type>, i64) {
    let mut lowered: Vec<LoweredItem> = Vec::with_capacity(element_ids.len());
    let mut elem_ty: Option<Type> = None;
    let mut literal_count: i64 = 0;
    for eid in element_ids {
        if let Expr::Spread { expr } = ctx.ast.get_expr(*eid) {
            let inner = *expr;
            let (op, v_ty, was_any) = lower_spread_source(ctx, inner);
            if was_any {
                // Materialized Arr<Any> — force the Any assembly path
                // regardless of what earlier items anchored.
                elem_ty = Some(Type::Any);
            }
            if let Type::Arr(arr_id) = v_ty
                && elem_ty.is_none()
            {
                elem_ty = Some(ctx.arr_layouts[arr_id.0 as usize]);
            }
            lowered.push(LoweredItem {
                op,
                src_eid: inner,
                is_spread: true,
                was_any,
            });
        } else {
            let v = ctx.lower_expr(*eid);
            let v_ty = ctx.operand_ty(&v);
            if elem_ty.is_none() {
                elem_ty = Some(v_ty);
            }
            literal_count += 1;
            lowered.push(LoweredItem {
                op: v,
                src_eid: *eid,
                is_spread: false,
                was_any: false,
            });
        }
    }
    (lowered, elem_ty, literal_count)
}

fn lower_spread_source(ctx: &mut LowerCtx<'_>, inner: ExprId) -> (Operand, Type, bool) {
    let mut v = ctx.lower_expr(inner);
    let mut v_ty = ctx.operand_ty(&v);
    // S134 — string spread `[...str]` unfolds per code unit. Substr
    // first materializes to owned Str.
    if matches!(v_ty, Type::Substr) {
        let cur_block = ctx.cur_block;
        let owned = ctx.f.append_inst(
            cur_block,
            InstKind::Call(ctx.intrinsics.substr_to_owned, vec![v]),
            Type::Str,
            None,
        );
        v = Operand::Value(owned);
        v_ty = Type::Str;
    }
    if matches!(v_ty, Type::Str) {
        let empty_sep = ctx.intern_string_literal("");
        let substr_arr_id = intern_arr_layout(ctx.arr_layouts, Type::Substr);
        let cur_block = ctx.cur_block;
        let substr_arr = ctx.f.append_inst(
            cur_block,
            InstKind::Call(ctx.intrinsics.str_split, vec![v, Operand::Value(empty_sep)]),
            Type::Arr(substr_arr_id),
            None,
        );
        let str_arr_id = intern_arr_layout(ctx.arr_layouts, Type::Str);
        v = ctx.materialize_arr_substr_to_str(Operand::Value(substr_arr), Type::Arr(str_arr_id));
        v_ty = Type::Arr(str_arr_id);
    }
    if matches!(v_ty, Type::Set) {
        let arr_any_id = intern_arr_layout(ctx.arr_layouts, Type::Any);
        v = crate::ssa_lower_arr_from_set::emit(ctx, v);
        v_ty = Type::Arr(arr_any_id);
    }
    // RFC 20260704 S5+ — `any` spread source: materialize through the
    // unified runtime iteration protocol into an owned Arr<Any> temp
    // (the assembler drops it after the extend).
    if matches!(v_ty, Type::Any) {
        let arr_any_id = intern_arr_layout(ctx.arr_layouts, Type::Any);
        v = crate::ssa_lower_arr_from_any::emit(ctx, v);
        v_ty = Type::Arr(arr_any_id);
        return (v, v_ty, true);
    }
    (v, v_ty, false)
}

fn build_items(
    ctx: &mut LowerCtx<'_>,
    lowered: &[LoweredItem],
    elem_is_refcounted: bool,
) -> Vec<Item> {
    let mut items: Vec<Item> = Vec::with_capacity(lowered.len());
    for li in lowered {
        if !elem_is_refcounted {
            ctx.consume_if_ident(li.src_eid);
        }
        items.push(if li.is_spread {
            Item::Spread(li.op)
        } else {
            Item::Lit(li.op)
        });
    }
    items
}

fn compute_total_length(ctx: &mut LowerCtx<'_>, items: &[Item], literal_count: i64) -> Operand {
    let mut total: Operand = Operand::ConstI64(literal_count);
    for it in items {
        if let Item::Spread(arr_op) = it {
            let cur_block = ctx.cur_block;
            let len = ctx.f.append_inst(
                cur_block,
                InstKind::Load(Type::I64, *arr_op, ARR_LEN_OFF),
                Type::I64,
                None,
            );
            let cur_block = ctx.cur_block;
            let summed = ctx.f.append_inst(
                cur_block,
                InstKind::BinOp(SsaBinOp::Add, total, Operand::Value(len)),
                Type::I64,
                None,
            );
            total = Operand::Value(summed);
        }
    }
    total
}

fn fill_arr_from_items(
    ctx: &mut LowerCtx<'_>,
    arr_ptr: crate::ssa::ValueId,
    items: Vec<Item>,
    elem_ty: Type,
    elem_is_refcounted: bool,
) {
    for it in items {
        match it {
            Item::Lit(v) => {
                let v = if elem_ty == Type::F64 && ctx.operand_ty(&v) == Type::I64 {
                    ctx.coerce_to_f64(v)
                } else {
                    v
                };
                let push_arg = ctx.raw_slot_arg(v);
                let cur_block = ctx.cur_block;
                ctx.f.append_void(
                    cur_block,
                    InstKind::Call(
                        ctx.intrinsics.arr_push_unchecked,
                        vec![Operand::Value(arr_ptr), push_arg],
                    ),
                );
                if elem_is_refcounted {
                    ctx.emit_rc_inc(v);
                }
            }
            Item::Spread(src) => {
                let cur_block = ctx.cur_block;
                let old_len = if elem_is_refcounted {
                    Some(ctx.f.append_inst(
                        cur_block,
                        InstKind::Load(Type::I64, Operand::Value(arr_ptr), ARR_LEN_OFF),
                        Type::I64,
                        None,
                    ))
                } else {
                    None
                };
                let cur_block = ctx.cur_block;
                ctx.f.append_void(
                    cur_block,
                    InstKind::Call(
                        ctx.intrinsics.arr_extend_unchecked,
                        vec![Operand::Value(arr_ptr), src],
                    ),
                );
                if let Some(old) = old_len {
                    let cur_block = ctx.cur_block;
                    let new_len = ctx.f.append_inst(
                        cur_block,
                        InstKind::Load(Type::I64, Operand::Value(arr_ptr), ARR_LEN_OFF),
                        Type::I64,
                        None,
                    );
                    ctx.emit_arr_rc_inc_range(
                        Operand::Value(arr_ptr),
                        Operand::Value(old),
                        Operand::Value(new_len),
                    );
                }
            }
        }
    }
}
