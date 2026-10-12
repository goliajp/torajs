//! `Expr::Nullish { lhs, rhs }` lowering pulled out of
//! [`crate::ssa_lower::lower_expr_inner`]'s match arm as chunk-73
//! of the decomp (chunks 1-72 = ... + `Expr::InstanceOf`
//! 4-layer dispatch).
//!
//! `lhs ?? rhs` (ES §13.4.2) — evaluate lhs once, branch on null,
//! result-slot store from either lhs (non-null) or rhs. Same shape
//! as Ternary but the cond comes from a pointer null-compare and
//! the lhs value is reused on the non-null path without re-eval.
//!
//! **Refcount (chunk 722)**: the generic-ptr join answers an OWNED
//! value — an owned-shape side (Call / New) transfers its fresh
//! reference into the result slot, a borrow-shape side (Ident /
//! Member) takes +1 in its branch tail — and the eid joins the
//! owned track (`expr_owned_shape`), so the consumer's single
//! release balances either way. The short-circuit lanes forward
//! the surviving side's ownership verdict instead.
//!
//! Four dispatch layers tried in source order:
//!
//! 1. **P3.5 Type::Any lhs** (typically from OptChain) — read the
//!    box's tag (offset 16); if `ANY_NULL=0` or `ANY_UNDEF=5`, use
//!    rhs; otherwise unbox lhs to rhs's SSA type and use it. The
//!    result is rhs's type. Per-type dispatch for the unbox value
//!    matches the box_to_any tag scheme:
//!    - rhs = Any: rc_inc lhs box, propagate as-is (no unbox-and-
//!      rebox round-trip).
//!    - I64/I32: take the box value field as-is.
//!    - F64: BitCastI64ToF64.
//!    - Bool: ICmp::Ne against 0.
//!    - refcounted heap: rc_inc + propagate.
//! 2. **S129-2 non-nullable lhs short-circuit** — `typed ?? any`
//!    (or `typed ?? typed`) where lhs's `check::Type` is NOT
//!    `Nullable`/`Null`/`Undefined`/`Any` short-circuits to lhs
//!    unconditionally per spec. Don't lower rhs (spec mandates rhs
//!    unevaluated) and don't slot; direct return. check.rs allows
//!    this pair narrowly by returning lhs_ty as the result type.
//!    Pre-fix the fall-through path would ICmp lhs_op (i64) vs
//!    ConstPtrNull, a typed-mismatch the typecheck wall previously
//!    blocked.
//! 3. **Always-nullish lhs short-circuit** — literal `null` /
//!    `undefined` lhs is statically nullish, so result is always
//!    rhs. The fall-through path below sizes the result slot by
//!    lhs's SSA type (e.g. `Type::Ptr` for null), then Loads back
//!    at that type — a typed rhs (e.g. String) would round-trip as
//!    the wrong SSA type and print as a raw pointer integer
//!    instead of the string content. Mirror the lhs_non_nullable
//!    short-circuit by lowering rhs at its own SSA type.
//! 4. **Generic Ptr lhs** with CondBr null-compare
//!    (`ptr`): the non-null branch stores lhs
//!    without re-eval; cond = (lhs == ConstPtrNull); null branch
//!    lowers rhs + stores. The join result is owned on both paths.
//!
//! Returns `Operand` directly (terminal arm — caller's
//! `Expr::Nullish` match arm bottoms out here).

use crate::ast::ExprId;
use crate::check::{self as check_mod};
use crate::ssa::{IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_nullish_any::lower_any_lhs;

mod ptr;

pub(crate) fn lower(ctx: &mut LowerCtx<'_>, eid: ExprId, lhs: ExprId, rhs: ExprId) -> Operand {
    // Cluster #4 — `o[k] ??= v` fingerprint: evaluate (and coerce)
    // the shared key once up front; the guard read and the branch-
    // side assign both consult the pin (see ssa_lower_logical's
    // `&&`/`||` twins). Unpinned after every join path below.
    // RFC 20260915 — the shared obj / index children lower once here,
    // receiver first, before the key pin runs the key's coercion.
    let shared = crate::ssa_lower_assign::pin_logical_target(ctx, lhs, rhs);
    let key_pin = ctx.pin_logical_assign_key(lhs, rhs);
    let out = lower_inner(ctx, eid, lhs, rhs);
    if let Some(owned) = key_pin {
        ctx.unpin_logical_assign_key(owned);
    }
    if let Some(shared) = shared {
        crate::ssa_lower_assign::unpin(ctx, shared, eid, &out);
    }
    out
}

fn lower_inner(ctx: &mut LowerCtx<'_>, eid: ExprId, lhs: ExprId, rhs: ExprId) -> Operand {
    let lhs_op = ctx.lower_expr(lhs);
    let lhs_ty = ctx.operand_ty(&lhs_op);
    if matches!(lhs_ty, Type::Any) {
        return lower_any_lhs(ctx, eid, lhs, lhs_op, rhs);
    }
    let lhs_check_ty = ctx.expr_types.get(&lhs).cloned();
    // RFC 20260708-typed-arr-oob-read chunk 2 — a possibly-sentinel
    // F64 lhs (number[] index read / alias) branches on the
    // undefined-NaN bits instead of the non-nullable short-circuit;
    // gated to a Number-typed rhs so the merged slot stays F64.
    if lhs_ty == Type::F64
        && crate::ssa_lower_undef_f64_source::is_undef_f64_source(ctx, lhs)
        && matches!(ctx.expr_types.get(&rhs), Some(check_mod::Type::Number))
    {
        return lower_f64_undefable_lhs(ctx, lhs_op, rhs);
    }
    // A pointer-shaped slot the checker types non-nullable can still
    // hold its undefined sentinel at run time — a `string[]` index
    // read past the end, a `pop()` off an empty array, a missing
    // `process.argv` entry: `argv[2] ?? "5"` forwarded the sentinel
    // and `Number` of it answered NaN. Such a source takes the
    // sentinel-aware pointer lane below; a join the checker widened
    // to `any` keeps its boxing lane.
    if !matches!(ctx.expr_types.get(&eid), Some(check_mod::Type::Any))
        && ((matches!(lhs_ty, Type::Str | Type::Substr)
            && (crate::ssa_lower_nullable_guard::is_nullable_str_source(ctx, lhs)
                || crate::ssa_lower_nullable_guard::is_undefable_substr_source(ctx, lhs)))
            || crate::ssa_lower_nullable_guard::holds_undef_cell(ctx, lhs, lhs_ty))
    {
        return ptr::lower(ctx, eid, lhs_op, lhs_ty, lhs, rhs);
    }
    if is_non_nullable(&lhs_check_ty) {
        // Chunk 722 — the short-circuit forwards lhs's value, so it
        // forwards lhs's ownership too: `mkStr(i) ?? d` discarded /
        // let-bound must release the Call's owned result.
        if ctx.join_arm_mints(lhs) {
            ctx.owned_member_reads.insert(eid);
        }
        return lhs_op;
    }
    if is_always_nullish(&lhs_check_ty) {
        // Chunk 722 — mirror: result IS rhs, ownership follows it
        // (the shape query runs AFTER the lowering so member-read
        // owned verdicts recorded during it are visible).
        let rhs_op = ctx.lower_expr(rhs);
        if ctx.join_arm_mints(rhs) {
            ctx.owned_member_reads.insert(eid);
        }
        return rhs_op;
    }
    // The checker widened this join to `Any` because the union of
    // the two sides has no name in its lattice (`o ?? "dflt"` on
    // `o: { a: number } | undefined`). It used to refuse those, so
    // this lane is new rather than a typed one being given up.
    //
    // The slot below is sized by lhs's own SSA type and the two
    // sides no longer share it, so widen — and widen by BOXING THE
    // LHS and taking the any-lhs lane above, rather than by growing
    // a second nullish test per shape. That lane's tag test is the
    // one question every shape answers the same way, and
    // `box_to_any_from_expr` is exactly the map onto it: the
    // generic cell, the F64 sentinel NaN and the string oddballs
    // all re-encode to ANY_UNDEF on the way in, which is why the
    // any world never has to know which family the slot came from.
    if matches!(ctx.expr_types.get(&eid), Some(check_mod::Type::Any)) && lhs_ty != Type::Any {
        let boxed = ctx.box_to_any_from_expr(lhs, lhs_op);
        return lower_any_lhs(ctx, eid, lhs, boxed, rhs);
    }
    ptr::lower(ctx, eid, lhs_op, lhs_ty, lhs, rhs)
}

fn is_non_nullable(check_ty: &Option<check_mod::Type>) -> bool {
    !matches!(
        check_ty,
        Some(check_mod::Type::Nullable(_))
            | Some(check_mod::Type::Null)
            | Some(check_mod::Type::Undefined)
            | Some(check_mod::Type::Any)
    )
}

fn is_always_nullish(check_ty: &Option<check_mod::Type>) -> bool {
    matches!(
        check_ty,
        Some(check_mod::Type::Null) | Some(check_mod::Type::Undefined)
    )
}

/// RFC 20260708-typed-arr-oob-read chunk 2 — `a[i] ?? d` where the
/// lhs may hold the undefined-NaN sentinel: bits-compare picks rhs
/// on the sentinel, lhs otherwise. Both sides are Number-typed
/// (caller gate), so the result slot is F64; an I64-width rhs
/// widens with coerce_to_f64.
fn lower_f64_undefable_lhs(ctx: &mut LowerCtx<'_>, lhs_op: Operand, rhs: ExprId) -> Operand {
    let bits = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::BitCastF64ToI64(lhs_op),
        Type::I64,
        None,
    );
    let is_undef = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(
            IPred::Eq,
            Operand::Value(bits),
            Operand::ConstI64(crate::ssa_lower_undef_f64_source::F64_UNDEF_SENTINEL_BITS as i64),
        ),
        Type::Bool,
        None,
    );
    let rhs_blk = ctx.f.add_block();
    let lhs_blk = ctx.f.add_block();
    let merge = ctx.f.add_block();
    let slot = ctx.alloca_in_entry(Type::F64, Some("__f64nullish"));
    let cb = ctx.cur_block;
    ctx.f.set_term(
        cb,
        Terminator::CondBr {
            cond: Operand::Value(is_undef),
            then_blk: rhs_blk,
            else_blk: lhs_blk,
        },
    );
    ctx.cur_block = rhs_blk;
    ctx.proofs.enter_cond();
    let rhs_raw = ctx.lower_expr(rhs);
    ctx.proofs.leave_cond();
    let rhs_f64 = ctx.coerce_to_f64(rhs_raw);
    let cur = ctx.cur_block;
    ctx.f
        .append_void(cur, InstKind::Store(rhs_f64, Operand::Value(slot), 0));
    ctx.f.set_term(cur, Terminator::Br(merge));
    ctx.cur_block = lhs_blk;
    ctx.f
        .append_void(lhs_blk, InstKind::Store(lhs_op, Operand::Value(slot), 0));
    ctx.f.set_term(lhs_blk, Terminator::Br(merge));
    ctx.cur_block = merge;
    let out = ctx.f.append_inst(
        merge,
        InstKind::Load(Type::F64, Operand::Value(slot), 0),
        Type::F64,
        None,
    );
    Operand::Value(out)
}
