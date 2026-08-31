//! `Array.prototype.splice` (ES §23.1.3.31) lowering — mirrors the
//! sibling [`crate::ssa_lower_tospliced`] carve so the splice dispatch
//! stops growing the 27k-line `ssa_lower.rs` god-fn and gains the
//! same (c) generic-receiver fallback toSpliced got at `689b1042`.
//!
//! Emit shape: receiver lowers to a `Type::Arr` pointer, then
//! `__torajs_arr_splice(arr, start, deleteCount)` mutates that array
//! in place (no realloc — only shrinks the live range) and returns
//! the removed sub-array as a fresh `Array<T>`. Unlike toSpliced,
//! no clone is made: splice is the mutating sibling.
//!
//! Receiver dispatch:
//! - (a) `Expr::Ident` bound to a `Type::Arr` local — load cur ptr
//!   from the slot.
//! - (b) `Expr::Ident` bound to a K.8 top-level refcount global —
//!   load cur ptr via `GlobalRef`.
//! - (c) any other expression that lowers to `Type::Arr` — literal
//!   arrays (`[1,2,3].splice(...)`), method chains, struct-field
//!   reads. Mutation in place is invisible to the user when the
//!   receiver has no binding (literal / call result), but the
//!   returned removed sub-array is still spec-correct, matching
//!   bun. The fresh-owned source array is dropped after the splice
//!   so its ARC balances.
//!
//! 0-2 args route to the shrink-only `__torajs_arr_splice`; 3+ args
//! (RFC 20260720-splice-insert knife 2) pack the `...items` tail
//! into a stack argv of raw elem-repr slots (each item through the
//! push-lane `coerce_push_value` + rc contract; an `Array<Any>`
//! receiver takes the `emit_any_item_bits` NaN-box lane instead) and
//! call `__torajs_arr_splice_items`. Returning `Some` short-circuits the
//! generic member-call dispatch in `ssa_lower.rs`; `None` falls
//! through to the "unsupported member call shape" panic site.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_lower(
    ctx: &mut LowerCtx,
    callee_eid: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    if let Some(v) = try_lower_local(ctx, callee_eid, args) {
        return Some(v);
    }
    if let Some(v) = try_lower_global(ctx, callee_eid, args) {
        return Some(v);
    }
    try_lower_generic(ctx, callee_eid, args)
}

fn try_lower_local(ctx: &mut LowerCtx, callee_eid: ExprId, args: &[ExprId]) -> Option<Operand> {
    let Expr::Member { obj: recv_id, name } = ctx.ast.get_expr(callee_eid) else {
        return None;
    };
    if name != "splice" {
        return None;
    }
    let Expr::Ident(recv_name) = ctx.ast.get_expr(*recv_id) else {
        return None;
    };
    let info = ctx.locals.get(recv_name).copied()?;
    let Type::Arr(_) = info.ty else {
        return None;
    };
    let arr_ty = info.ty;
    let cur_arr = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(arr_ty, Operand::Value(info.slot), 0),
        arr_ty,
        None,
    );
    Some(emit_splice_return(ctx, arr_ty, cur_arr, args))
}

fn try_lower_global(ctx: &mut LowerCtx, callee_eid: ExprId, args: &[ExprId]) -> Option<Operand> {
    let Expr::Member { obj: recv_id, name } = ctx.ast.get_expr(callee_eid) else {
        return None;
    };
    if name != "splice" {
        return None;
    }
    let Expr::Ident(recv_name) = ctx.ast.get_expr(*recv_id) else {
        return None;
    };
    if ctx.locals.get(recv_name).is_some() {
        return None;
    }
    let slot_ty = ctx.globals.get(recv_name).copied()?;
    let Type::Arr(_) = slot_ty else {
        return None;
    };
    let arr_ty = slot_ty;
    let slot_ptr = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::GlobalRef(recv_name.clone()),
        Type::Ptr,
        None,
    );
    let cur_arr = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(arr_ty, Operand::Value(slot_ptr), 0),
        arr_ty,
        None,
    );
    Some(emit_splice_return(ctx, arr_ty, cur_arr, args))
}

/// (c) Generic receiver — literal array, method chain, struct field
/// read, etc. The receiver expression's lowered value is the array
/// we splice in place. Fresh-owned receivers (literal arrays, call
/// results) get dropped after the splice to balance ARC; the returned
/// removed sub-array owns its own refs.
fn try_lower_generic(ctx: &mut LowerCtx, callee_eid: ExprId, args: &[ExprId]) -> Option<Operand> {
    let Expr::Member { obj: recv_id, name } = ctx.ast.get_expr(callee_eid) else {
        return None;
    };
    if name != "splice" {
        return None;
    }
    let recv_eid = *recv_id;
    let recv_op = ctx.lower_expr(recv_eid);
    let recv_ty = ctx.operand_ty(&recv_op);
    let Type::Arr(_) = recv_ty else {
        return None;
    };
    let cur_arr = match recv_op {
        Operand::Value(v) => v,
        _ => return None,
    };
    let removed = emit_splice_return(ctx, recv_ty, cur_arr, args);
    if ctx.expr_is_fresh_owned(recv_eid) {
        ctx.emit_drop_value(Operand::Value(cur_arr), recv_ty);
    }
    Some(removed)
}

/// Shared body: emit `arr_splice(arr, start, deleteCount)` and
/// return the removed sub-array operand.
///
/// Arity defaults follow ES §23.1.3.31 step 4-6:
/// - 0-arg `splice()` — `start = 0`, `deleteCount = 0`
/// - 1-arg `splice(start)` — `deleteCount = len - actualStart`
/// - 2-arg `splice(start, deleteCount)` — passed through
///
/// The `__torajs_arr_splice` helper clamps `deleteCount` to
/// `len - actualStart` internally, so 1-arg uses `i64::MAX` as a
/// pass-through sentinel ("delete everything from start"). 0-arg
/// spec says `deleteCount = 0` (skip step 5.b altogether), which
/// `ConstI64(0)` produces without changing the helper.
fn emit_splice_return(
    ctx: &mut LowerCtx,
    arr_ty: Type,
    cur_arr: crate::ssa::ValueId,
    args: &[ExprId],
) -> Operand {
    // §23.1.3.31 step 8 ArraySpeciesCreate (the removed product) —
    // constructor-face guard (RFC 20260713 blade 3).
    ctx.emit_arr_species_guard(Operand::Value(cur_arr));
    let start = if args.is_empty() {
        Operand::ConstI64(0)
    } else {
        // §23.1.3.31 step 3 ToIntegerOrInfinity(start). The helper's
        // params are i64, so a raw `lower_expr` handed an `any` its
        // box bits as a position — `a.splice(i, 1)` for an `i: any`
        // removed the wrong element and said nothing.
        ctx.lower_to_index_operand(args[0])
    };
    // S237 — `arr.splice(start, undefined)` per ES §23.1.3.31 step 7:
    // ToIntegerOrInfinity(undefined) = 0, so an explicit-undefined
    // deleteCount yields 0 removal (distinct from the omitted-arg
    // 1-arg shape's `len - actualStart` spec default). Detect the
    // typed-Undefined operand and substitute ConstI64(0) before the
    // helper Call.
    let delete_count = match args.len() {
        0 => Operand::ConstI64(0),
        1 => Operand::ConstI64(i64::MAX),
        _ => {
            if matches!(
                ctx.expr_types.get(&args[1]),
                Some(crate::check::Type::Undefined)
            ) {
                Operand::ConstI64(0)
            } else {
                ctx.lower_to_index_operand(args[1])
            }
        }
    };
    if args.len() > 2 {
        return emit_splice_items(ctx, arr_ty, cur_arr, start, delete_count, &args[2..]);
    }
    let removed = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.arr_splice,
            vec![Operand::Value(cur_arr), start, delete_count],
        ),
        arr_ty,
        None,
    );
    // 刀 6 G9a — the kernel's length-lock entry guard records a
    // catchable TypeError (§23.1.3.31 step 24).
    ctx.emit_throw_check(None);
    Operand::Value(removed)
}

/// RFC 20260720-splice-insert knife 2 — the `...items` insert form.
/// Each item runs the push-lane `coerce_push_value` (elem-repr
/// coercion incl. the Any→scalar/Str unbox the checker wedge admits)
/// and the push rc contract (borrowed refcounted values take the
/// receiver's stake here; owned mints hand their +1 off, substr/Any
/// coerce lanes are fresh rc=1 hand-offs). The coerced raw slots pack
/// into an entry-block stack argv; the kernel moves bits only.
///
/// An `Array<Any>` receiver (8-byte NaN-box slots) takes the
/// [`emit_any_item_bits`] lane instead: each item arrives as
/// already-encoded box bits carrying the slot's stake, matching the
/// kernel's raw-bit-store contract — unlike push_any / index-assign,
/// which hand (tag, value) pairs to re-encoding helpers.
pub(crate) fn emit_splice_items(
    ctx: &mut LowerCtx,
    arr_ty: Type,
    cur_arr: crate::ssa::ValueId,
    start: Operand,
    delete_count: Operand,
    items: &[ExprId],
) -> Operand {
    let Type::Arr(arr_id) = arr_ty else {
        unreachable!("splice items on non-Arr receiver");
    };
    let elem_ty = ctx.arr_layouts[arr_id.0 as usize];
    let argv = ctx.f.append_inst(
        crate::ssa::BlockId(0),
        InstKind::AllocaBytes((items.len() * 8) as u64),
        Type::Ptr,
        Some("__splice_items"),
    );
    for (k, &item_eid) in items.iter().enumerate() {
        let val = if matches!(elem_ty, Type::Any) {
            emit_any_item_bits(ctx, item_eid)
        } else {
            let (val, owned_from_coerce) =
                crate::ssa_lower_call_arr_push::coerce_push_value(ctx, item_eid, elem_ty);
            if elem_ty.is_refcounted() && !owned_from_coerce {
                ctx.emit_rc_inc(val.clone());
                ctx.release_owned_temp(item_eid, &val);
            }
            val
        };
        let raw = ctx.raw_slot_arg(val);
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(raw, Operand::Value(argv), (k * 8) as u64),
        );
    }
    let removed = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.arr_splice_items,
            vec![
                Operand::Value(cur_arr),
                start,
                delete_count,
                Operand::Value(argv),
                Operand::ConstI64(items.len() as i64),
            ],
        ),
        arr_ty,
        None,
    );
    // 刀 6 G9a — the kernel's length-lock entry guard records a
    // catchable TypeError (§23.1.3.31 step 24).
    ctx.emit_throw_check(None);
    Operand::Value(removed)
}

/// `Array<Any>` receiver — encode one `...items` item into NaN-box
/// bits carrying the slot's stake (the kernel stores the bits raw,
/// so each slot must arrive stake-balanced). Ledger per item shape
/// (the chunk-733 inc + release idiom, uniform with the typed branch
/// above; `release_owned_temp` also catches lifted-arrow minted
/// closures that the chunk-565 `transfers` predicate misses):
/// - `Any` item: bits pass through; the slot's +1 goes through the
///   NaN-box-gated `any_box_rc_inc` (immediates no-op), then owned
///   temps release their own ref — net transfer.
/// - refcounted (Str included): every box arm is a pure encode with
///   zero rc traffic (chunk 753 tag-4; rotation 184 made the
///   Str-slot helper rc-neutral, retiring the "+1 rides the box"
///   claim this doc used to carry — rotation 185 stake audit), so
///   the slot's +1 is an explicit header inc (no-op on NULL / the
///   sentinels); owned / minted temps release — net transfer. (A
///   short string literal takes the `box_to_any_from_expr`
///   compile-time ShortStr encode instead — an immediate, zero rc
///   traffic; the inc no-ops on the interned static cell.)
/// - scalars / null / undefined / dynobj Ptr: pure encode, no rc
///   traffic (a fresh dynobj literal's +1 rides the box, same as
///   push_any's Ptr arm).
///
/// Boxing goes through the expr-aware `box_to_any_from_expr` — an
/// `undefined` literal lowers to the same `ConstPtrNull` as `null`
/// and only the frontend type picks ANY_UNDEF=5 over ANY_NULL=0.
fn emit_any_item_bits(ctx: &mut LowerCtx, item_eid: ExprId) -> Operand {
    let v_raw = ctx.lower_expr(item_eid);
    let v_ty = ctx.operand_ty(&v_raw);
    match v_ty {
        Type::Any => {
            ctx.emit_owned_result_inc(v_raw.clone(), Type::Any);
            ctx.release_owned_temp(item_eid, &v_raw);
            v_raw
        }
        _ if v_ty.is_refcounted() => {
            ctx.emit_rc_inc(v_raw.clone());
            let bits = ctx.box_to_any_from_expr(item_eid, v_raw.clone());
            ctx.release_owned_temp(item_eid, &v_raw);
            bits
        }
        _ => ctx.box_to_any_from_expr(item_eid, v_raw),
    }
}
