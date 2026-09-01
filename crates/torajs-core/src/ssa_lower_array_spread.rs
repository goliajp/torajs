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
    /// Rotation 543 — `op` is an array THIS lane minted (a string, a
    /// Substr or a Set walked into a fresh `Arr`), not a value the
    /// program can still name. It has no other owner, so the
    /// assembler owes it an unconditional drop; `release_owned_temp`
    /// cannot serve here because the source ExprId describes the
    /// string, not the array that replaced it.
    pub(crate) minted: bool,
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
    release_spread_sources(ctx, &lowered);
    Operand::Value(arr_ptr)
}

/// Rotation 543 — the typed assembler copies out of each spread
/// source and never gave any of them back. `arr_extend_unchecked`
/// reads the source's slots and `emit_adopt_copied_range` takes its
/// own +1 per copied element, so the source's stake is entirely the
/// caller's to settle — and nothing did.
///
/// Two shapes, and they cannot share one call: a MINTED array (the
/// string / Substr / Set walks) has no other owner and is dropped
/// unconditionally, while a source the program still names is
/// released only when the expression was an owned temp. The any
/// assembler already does the same accounting behind `was_any`.
fn release_spread_sources(ctx: &mut LowerCtx<'_>, lowered: &[LoweredItem]) {
    for li in lowered {
        if !li.is_spread || li.was_any {
            continue;
        }
        let ty = ctx.operand_ty(&li.op);
        if li.minted {
            ctx.emit_drop_value(li.op.clone(), ty);
        } else {
            ctx.release_owned_temp(li.src_eid, &li.op);
        }
    }
}

fn lower_spread_elements(
    ctx: &mut LowerCtx<'_>,
    element_ids: &[ExprId],
) -> (Vec<LoweredItem>, Option<Type>, i64) {
    let mut lowered: Vec<LoweredItem> = Vec::with_capacity(element_ids.len());
    let mut elem_ty: Option<Type> = None;
    let mut literal_count: i64 = 0;
    // Rotation 543 — what each item says the literal's element type
    // should be. `elem_ty` below still records the FIRST answer, which
    // is what the typed assembler has always used; this records ALL of
    // them so a disagreement can be seen at the end.
    let mut contributed: Vec<Type> = Vec::new();
    for eid in element_ids {
        if let Expr::Spread { expr } = ctx.ast.get_expr(*eid) {
            let inner = *expr;
            let (op, v_ty, was_any, minted) = lower_spread_source(ctx, inner);
            if was_any {
                // Materialized Arr<Any> — force the Any assembly path
                // regardless of what earlier items anchored.
                elem_ty = Some(Type::Any);
                contributed.push(Type::Any);
            } else if let Type::Arr(arr_id) = v_ty {
                // A spread of an `Arr<Substr>` lands as owned strings
                // (a view does not leave its split block — rotation
                // 468), so the literal is `Arr<Str>`, never `Arr<Substr>`.
                let out_id = ctx.copied_arr_layout(arr_id);
                let t = ctx.arr_layouts[out_id.0 as usize];
                contributed.push(t);
                if elem_ty.is_none() {
                    elem_ty = Some(t);
                }
            }
            lowered.push(LoweredItem {
                op,
                src_eid: inner,
                is_spread: true,
                was_any,
                minted,
            });
        } else {
            let v = ctx.lower_expr(*eid);
            let v_ty = ctx.operand_ty(&v);
            // A substring VIEW element (`[...a, s[1]]`) is stored as an
            // owned copy — the same rule the plain literal lane applies
            // (`materialize_substr_elem`): the fresh copy is the
            // element, a fresh-mint view is released here, a borrow
            // stays with its owner (rotation 468).
            let (v, v_ty) = if v_ty == Type::Substr {
                let owned = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Call(ctx.intrinsics.substr_to_owned, vec![v.clone()]),
                    Type::Str,
                    None,
                );
                if ctx.expr_transfers_ownership(*eid) {
                    ctx.emit_drop_value(v, Type::Substr);
                }
                (Operand::Value(owned), Type::Str)
            } else {
                (v, v_ty)
            };
            contributed.push(v_ty);
            if elem_ty.is_none() {
                elem_ty = Some(v_ty);
            }
            literal_count += 1;
            lowered.push(LoweredItem {
                op: v,
                src_eid: *eid,
                is_spread: false,
                was_any: false,
                minted: false,
            });
        }
    }
    // Rotation 543 — the first item used to decide the element type
    // for ALL of them, and nothing ever asked the rest whether they
    // agreed. `[...[1, 2], ...["a"]]` printed `4309125696`, a Str
    // pointer read through an I64 slot; `[...[1, 2], ..."ab"]` printed
    // `2.14e-314`, the same pointer read as an f64; and the reverse
    // orders, which read a small integer as a pointer, were
    // **exit 139** — `[...["a"], ...[1, 2]]` is three tokens long.
    // A plain literal element counts too: `[...[1, 2], "a"]` has only
    // one spread in it and printed a pointer.
    //
    // When the items disagree the literal is an `Array<Any>`, which is
    // what the spelling means and what the any assembler builds.
    if !elem_types_agree(&contributed) {
        elem_ty = Some(Type::Any);
    }
    (lowered, elem_ty, literal_count)
}

/// Whether every item can live in one typed array. Numbers are one
/// bucket on purpose: `[...[1, 2], ...[3.5]]` has always worked
/// because `num_width` widens the anon slots together, and routing it
/// through the any lane would tax a path that is fast precisely
/// because it is not boxed.
fn elem_types_agree(tys: &[Type]) -> bool {
    let Some(first) = tys.first() else {
        return true;
    };
    if tys
        .iter()
        .all(|t| matches!(t, Type::I64 | Type::I32 | Type::F64))
    {
        return true;
    }
    tys.iter().all(|t| t == first)
}

fn lower_spread_source(ctx: &mut LowerCtx<'_>, inner: ExprId) -> (Operand, Type, bool, bool) {
    let mut v = ctx.lower_expr(inner);
    let mut v_ty = ctx.operand_ty(&v);
    // Set once a walk replaces the user's value with a fresh array —
    // see `LoweredItem::minted`.
    let mut minted = false;
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
            InstKind::Call(ctx.intrinsics.substr_to_owned, vec![v.clone()]),
            Type::Str,
            None,
        );
        // Same rule as the non-spread element arm: a fresh-mint view
        // is released here, a borrow stays with its owner.
        ctx.release_owned_temp(inner, &v);
        v = Operand::Value(owned);
        v_ty = Type::Str;
        minted = true;
    }
    if matches!(v_ty, Type::Str) {
        // The same intrinsic `Array.from(str)` lowers to — one
        // code-point-correct walk, already declared, and it hands back
        // `Arr<Str>` directly instead of a split-then-materialize pair.
        let str_arr_id = intern_arr_layout(ctx.arr_layouts, Type::Str);
        let cur_block = ctx.cur_block;
        let arr = ctx.f.append_inst(
            cur_block,
            InstKind::Call(ctx.intrinsics.arr_from_string, vec![v.clone()]),
            Type::Arr(str_arr_id),
            None,
        );
        // The walk reads the string and answers a fresh array, so the
        // string's own stake is still ours: an owned source temp (or
        // the Str the Substr arm just minted) is released here.
        if minted {
            ctx.emit_drop_value(v, Type::Str);
        } else {
            ctx.release_owned_temp(inner, &v);
        }
        v = Operand::Value(arr);
        v_ty = Type::Arr(str_arr_id);
        minted = true;
    }
    if matches!(v_ty, Type::Set) {
        let arr_any_id = intern_arr_layout(ctx.arr_layouts, Type::Any);
        let src = v.clone();
        v = crate::ssa_lower_arr_from_set::emit(ctx, v);
        ctx.release_owned_temp(inner, &src);
        v_ty = Type::Arr(arr_any_id);
        minted = true;
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
        return (v, v_ty, true, minted);
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
        return (v, v_ty, true, minted);
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
        return (v, v_ty, true, minted);
    }
    (v, v_ty, false, minted)
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
                    // Type-aware (rotation 412): an Any elem is a
                    // NaN-box encoding, not a header ptr — the gated
                    // inc no-ops immediates instead of dereferencing
                    // their payload.
                    ctx.emit_owned_result_inc(v, elem_ty);
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
                    // The new slots were copied from `src`: a spread of
                    // an `Arr<Substr>` lands as owned strings (views do
                    // not leave their split block — rotation 468), so
                    // the source's element type picks the adopt walk,
                    // not the literal's.
                    let src_elem_ty = match ctx.operand_ty(&src) {
                        Type::Arr(src_id) => ctx.arr_layouts[src_id.0 as usize],
                        _ => elem_ty,
                    };
                    ctx.emit_adopt_copied_range(
                        Operand::Value(arr_ptr),
                        src_elem_ty,
                        Operand::Value(old),
                        Operand::Value(new_len),
                    );
                }
            }
        }
    }
}
