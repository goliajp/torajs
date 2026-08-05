//! Spread-path array-literal lowering split out of
//! [`crate::ssa_lower_array`] (B1 file-size split — the no-spread /
//! alloc half stays in the parent). Pre-computes total length at
//! runtime, allocs once, fills via `arr_push_unchecked` +
//! `arr_extend_unchecked`; see the parent module doc for the full
//! dispatch story.

use crate::ast::{Expr, ExprId};
use crate::ssa::{BinOp as SsaBinOp, InstKind, Operand, Type};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx, intern_arr_layout};

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

pub(crate) fn lower_spread(ctx: &mut LowerCtx<'_>, element_ids: &[ExprId], eid: ExprId) -> Operand {
    let (lowered, elem_ty, literal_count) = lower_spread_elements(ctx, element_ids);
    // Any element type assembles from the ALREADY-lowered operands —
    // re-lowering the ExprIds (the pre-S5+ shape) would double-emit
    // spread source side effects (e.g. `[...m.values()]` minting the
    // iterator twice).
    if matches!(elem_ty, Some(Type::Any)) {
        return crate::ssa_lower_arr_from_any::assemble_any_spread(ctx, lowered, literal_count);
    }
    let items = build_items(&lowered);
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
    // String spread `[...str]` unfolds per Unicode **code point**, not
    // per code unit: §13.2.4.1 runs the spread through GetIterator, and
    // the String iterator (§22.1.5) yields code points, so `[..."👋a"]`
    // has two elements rather than three.
    //
    // Pre-fix this split on `""`, which is code-unit-correct for
    // `String.prototype.split` and wrong here — a surrogate pair came
    // back as its two halves. `for (const c of s)` and
    // `Array.from(s)` were already right, so the bug needed a string
    // outside the BMP *and* the spread spelling specifically.
    //
    // Substr first materializes to owned Str.
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
        // The same intrinsic `Array.from(str)` lowers to — one
        // code-point-correct walk, already declared, and it hands back
        // `Arr<Str>` directly instead of a split-then-materialize pair.
        let str_arr_id = intern_arr_layout(ctx.arr_layouts, Type::Str);
        let cur_block = ctx.cur_block;
        let arr = ctx.f.append_inst(
            cur_block,
            InstKind::Call(ctx.intrinsics.arr_from_string, vec![v]),
            Type::Arr(str_arr_id),
            None,
        );
        v = Operand::Value(arr);
        v_ty = Type::Arr(str_arr_id);
    }
    if matches!(v_ty, Type::Set) {
        let arr_any_id = intern_arr_layout(ctx.arr_layouts, Type::Any);
        v = crate::ssa_lower_arr_from_set::emit(ctx, v);
        v_ty = Type::Arr(arr_any_id);
    }
    // `[...map]` / `[...m.keys()/.values()/.entries()]` /
    // `[...set.values()]` — a statically-typed Map or iterator cell.
    // The unified runtime iteration protocol (`arr_from_any::emit`)
    // already drives these behind the erased `any` tag, so box the
    // heap source (tag-4 ANY_HEAP, an rc-neutral pure encode) and
    // route it through the same materializer. Mirrors the `any` arm
    // below (`was_any = true`): `emit` yields an owned `Arr<Any>`,
    // and `release_owned_temp` settles the source temp's own stake
    // (owned `m.keys()` call → dropped; borrowed Ident/Member → not).
    if matches!(v_ty, Type::Map | Type::MapIter | Type::ArrIter) {
        let arr_any_id = intern_arr_layout(ctx.arr_layouts, Type::Any);
        let boxed = ctx.box_to_any(v.clone());
        let materialized = crate::ssa_lower_arr_from_any::emit(ctx, boxed);
        ctx.release_owned_temp(inner, &v);
        v = materialized;
        v_ty = Type::Arr(arr_any_id);
        return (v, v_ty, true);
    }
    // RFC 20260725-getiterator-getmethod knife 5 — a class instance
    // (a generator object, a class declaring `[Symbol.iterator]`)
    // takes the same route as a Map: box the cell and let §7.4.2
    // GetIterator decide at runtime what it is. Before knife 2 there
    // was no lookup to make, so the checker refused these outright.
    if matches!(v_ty, Type::Obj(_)) {
        let arr_any_id = intern_arr_layout(ctx.arr_layouts, Type::Any);
        let boxed = ctx.box_to_any(v.clone());
        let materialized = crate::ssa_lower_arr_from_any::emit(ctx, boxed);
        ctx.release_owned_temp(inner, &v);
        v = materialized;
        v_ty = Type::Arr(arr_any_id);
        return (v, v_ty, true);
    }
    // RFC 20260704 S5+ — `any` spread source: materialize through the
    // unified runtime iteration protocol into an owned Arr<Any> temp
    // (the assembler drops it after the extend).
    if matches!(v_ty, Type::Any) {
        let arr_any_id = intern_arr_layout(ctx.arr_layouts, Type::Any);
        let materialized = crate::ssa_lower_arr_from_any::emit(ctx, v.clone());
        // An owned any source (`[...s.split("b")]` — Call / New /
        // OptCall shapes) is only borrowed by the iteration protocol;
        // release it after the materialization or every spread strands
        // the product's +1 (mirrors the any-call receiver account).
        // Borrow shapes (Ident / Member) self-gate false.
        ctx.release_owned_temp(inner, &v);
        v = materialized;
        v_ty = Type::Arr(arr_any_id);
        return (v, v_ty, true);
    }
    (v, v_ty, false)
}

fn build_items(lowered: &[LoweredItem]) -> Vec<Item> {
    lowered
        .iter()
        .map(|li| {
            if li.is_spread {
                Item::Spread(li.op)
            } else {
                Item::Lit(li.op)
            }
        })
        .collect()
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
