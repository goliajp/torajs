//! The three `emit_*_guard` emitters, moved out of
//! [`crate::ssa_lower_nullable_guard`] whole.
//!
//! That file answers one question — can this value be holding a
//! nullish in-band spelling right now — in as many forms as there
//! are slot families. These three do not answer it; they act on
//! the answer, by arming the catchable TypeError a member read
//! needs in front of it. Predicates on one side of the cut,
//! emission on the other, which is also the only reason any of
//! this needs a `&mut LowerCtx`.
//!
//! Pure position — the three items are byte-for-byte what they
//! were, and the parent re-exports them so no call site moves.

use crate::ast::{Expr, ExprId};
use crate::ssa::{FuncId, IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_binop_null_undef::UNDEF_CELL_SYM;
use crate::ssa_lower_intrinsics_str_b::STR_UNDEF_CELL_SYM;
use crate::ssa_lower_intrinsics_substr::SUBSTR_UNDEF_CELL_SYM;
use crate::ssa_lower_nullable_guard::{
    is_nullable_arr_source, is_nullable_str_source, is_undefable_heap_source,
    is_undefable_substr_source,
};

/// Emit the null guard when `obj` is a nullable-arr source or an
/// undefable-heap source (RFC 20260722 chunk B — an `Arr[]`
/// find/findLast miss answers the generic undefined cell, and the
/// kernel throws on it too); no-op otherwise. `arr_val` must be the
/// already-lowered receiver.
pub(crate) fn emit_nullable_arr_guard(ctx: &mut LowerCtx<'_>, obj: ExprId, arr_val: &Operand) {
    if !is_nullable_arr_source(ctx, obj) && !is_undefable_heap_source(ctx, obj) {
        return;
    }
    let raiser = ctx.intrinsics.arr_null_check;
    emit_proven_nullish_guard(ctx, obj, arr_val, raiser, &[UNDEF_CELL_SYM]);
}

/// Emit the heap nullish guard when `obj` is an undefable-heap
/// source; no-op otherwise. `obj_val` must be the already-lowered
/// receiver. Same shape as [`emit_nullable_str_guard`]:
/// `heap_nullish_check` arms a catchable TypeError on NULL or the
/// generic undefined cell, the throw-check right after diverts
/// before the member read dereferences.
pub(crate) fn emit_undefable_heap_guard(ctx: &mut LowerCtx<'_>, obj: ExprId, obj_val: &Operand) {
    if !is_undefable_heap_source(ctx, obj) {
        return;
    }
    let raiser = ctx.intrinsics.heap_nullish_check;
    emit_proven_nullish_guard(ctx, obj, obj_val, raiser, &[UNDEF_CELL_SYM]);
}

/// The guard, minus what is already proven: nothing for a value its
/// producing lane proved live or a binding an earlier guard on this
/// path settled; otherwise the inline test against NULL and the
/// `sentinels` the slot family spells `undefined` with, after which
/// a provable binding is settled for the rest of the path.
fn emit_proven_nullish_guard(
    ctx: &mut LowerCtx<'_>,
    obj: ExprId,
    v: &Operand,
    raiser: FuncId,
    sentinels: &[&str],
) {
    if ctx.proofs.nonnull_exprs.contains(&obj) {
        return;
    }
    let binding = provable_binding(ctx, obj);
    if binding
        .as_deref()
        .is_some_and(|n| ctx.proofs.let_is_nonnull(n))
    {
        return;
    }
    emit_inline_nullish_guard(ctx, v, raiser, sentinels);
    if let Some(n) = binding {
        ctx.proofs.note_let_nonnull(&n);
    }
}

/// The fn-local binding `eid` reads, when a guard on it can be
/// remembered: a plain local that this fn's closures do not capture
/// and, in the program's main body, one declared inside a block. A
/// capture box, a captured binding and a top-level binding can all be
/// written by a call in between.
fn provable_binding(ctx: &LowerCtx<'_>, eid: ExprId) -> Option<String> {
    match ctx.ast.get_expr(eid) {
        Expr::Ident(n)
            if ctx
                .locals
                .get(n)
                .is_some_and(|l| !l.boxed && (!ctx.is_main_fn || l.scope_depth > 0))
                && !ctx.globals.contains_key(n)
                && !ctx.escape_captured_lets.contains(n) =>
        {
            Some(n.clone())
        }
        Expr::As { expr, .. } => provable_binding(ctx, *expr),
        _ => None,
    }
}

/// `v` is NULL or one of the `sentinels` (the immortal cells a slot
/// family spells `undefined` with) → `raiser(v)` arms the TypeError
/// with the wording for that spelling and the throw check diverts; a
/// live cell continues. The compares stay inline so the hot path
/// carries no call and no throw-flag load, and the guard on a
/// loop-invariant receiver is a branch the loop optimizer can hoist.
pub(crate) fn emit_inline_nullish_guard(
    ctx: &mut LowerCtx<'_>,
    v: &Operand,
    raiser: FuncId,
    sentinels: &[&str],
) {
    let cold = ctx.f.add_block();
    let live = ctx.f.add_block();
    let mut cond = ctx.cmp(IPred::Eq, *v, Operand::ConstPtrNull);
    for sym in sentinels {
        let next = ctx.f.add_block();
        let cb = ctx.cur_block;
        ctx.f.set_term(
            cb,
            Terminator::CondBr {
                cond,
                then_blk: cold,
                else_blk: next,
            },
        );
        ctx.cur_block = next;
        let cell = ctx
            .f
            .append_inst(next, InstKind::GlobalRef(sym.to_string()), Type::Ptr, None);
        cond = ctx.cmp(IPred::Eq, *v, Operand::Value(cell));
    }
    let cb = ctx.cur_block;
    ctx.f.set_term(
        cb,
        Terminator::CondBr {
            cond,
            then_blk: cold,
            else_blk: live,
        },
    );
    ctx.cur_block = cold;
    emit_nullish_raise(ctx, raiser, *v);
    ctx.cur_block = live;
}

/// On a path where `v` is known nullish: arm the TypeError through
/// `raiser` and propagate it. The kernel always records a throw for a
/// nullish argument, so the fall-through after the check is
/// unreachable and is sealed as such.
pub(crate) fn emit_nullish_raise(ctx: &mut LowerCtx<'_>, raiser: FuncId, v: Operand) {
    let cb = ctx.cur_block;
    ctx.f.append_void(cb, InstKind::Call(raiser, vec![v]));
    ctx.emit_throw_check(None);
    let tail = ctx.cur_block;
    ctx.f.set_term(tail, Terminator::Unreachable);
}

/// Emit the heap nullish guard when `v`'s slot may be holding the
/// generic immortal cell right now — the SOURCE and the TYPE both,
/// which is the pair [`crate::ssa_lower_nullable_guard::holds_undef_cell`]
/// answers.
///
/// [`emit_undefable_heap_guard`] asks the source alone, which is
/// right at a site that already knows its value is pointer-shaped
/// (a Map / Date / regex receiver). A site that sees several slot
/// families needs the type question too, or the guard fires on one
/// whose `undefined` is a different bit pattern entirely — an F64
/// slot spells it NaN, a Str slot with an oddball.
pub(crate) fn emit_undef_cell_guard(ctx: &mut LowerCtx<'_>, eid: ExprId, v: &Operand) {
    let ty = ctx.operand_ty(v);
    if crate::ssa_lower_nullable_guard::holds_undef_cell(ctx, eid, ty) {
        emit_undefable_heap_guard(ctx, eid, v);
    }
}

/// Emit the Str null guard when `obj` is a nullable-str source or
/// an undefable-substr source (string index read — its slot may
/// hold the Substr-shaped sentinel); no-op otherwise. `str_val`
/// must be the already-lowered receiver. Same shape as
/// [`emit_nullable_arr_guard`], against the two cells a string slot
/// spells `undefined` with (the Str sentinel, and the Substr-shaped
/// one an out-of-range index read answers): `str_null_check` arms the
/// catchable TypeError on the cold path only.
pub(crate) fn emit_nullable_str_guard(ctx: &mut LowerCtx<'_>, obj: ExprId, str_val: &Operand) {
    if !is_nullable_str_source(ctx, obj) && !is_undefable_substr_source(ctx, obj) {
        return;
    }
    let raiser = ctx.intrinsics.str_null_check;
    emit_proven_nullish_guard(
        ctx,
        obj,
        str_val,
        raiser,
        &[STR_UNDEF_CELL_SYM, SUBSTR_UNDEF_CELL_SYM],
    );
}
