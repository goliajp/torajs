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
    emit_proven_nullish_guard(ctx, obj, arr_val, raiser);
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
    emit_proven_nullish_guard(ctx, obj, obj_val, raiser);
}

/// The guard, minus what is already proven: nothing for a value its
/// producing lane proved live or a binding an earlier guard on this
/// path settled; otherwise the inline test, after which a provable
/// binding is settled for the rest of the path.
fn emit_proven_nullish_guard(ctx: &mut LowerCtx<'_>, obj: ExprId, v: &Operand, raiser: FuncId) {
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
    emit_inline_nullish_guard(ctx, v, raiser);
    if let Some(n) = binding {
        ctx.proofs.note_let_nonnull(&n);
    }
}

/// The fn-local binding `eid` reads, when a guard on it can be
/// remembered: a plain local of a non-main fn that this fn's closures
/// do not capture. A capture box, a captured binding and a top-level
/// binding can all be written by a call in between.
fn provable_binding(ctx: &LowerCtx<'_>, eid: ExprId) -> Option<String> {
    if ctx.is_main_fn {
        return None;
    }
    match ctx.ast.get_expr(eid) {
        Expr::Ident(n)
            if ctx.locals.get(n).is_some_and(|l| !l.boxed)
                && !ctx.globals.contains_key(n)
                && !ctx.escape_captured_lets.contains(n) =>
        {
            Some(n.clone())
        }
        Expr::As { expr, .. } => provable_binding(ctx, *expr),
        _ => None,
    }
}

/// `v` is NULL or the generic undefined cell → `raiser(v)` arms the
/// TypeError with the wording for that spelling and the throw check
/// diverts; a live cell continues. The two compares stay inline so
/// the hot path carries no call and no throw-flag load.
pub(crate) fn emit_inline_nullish_guard(ctx: &mut LowerCtx<'_>, v: &Operand, raiser: FuncId) {
    let cold = ctx.f.add_block();
    let cell_test = ctx.f.add_block();
    let live = ctx.f.add_block();
    let is_null = ctx.cmp(IPred::Eq, v.clone(), Operand::ConstPtrNull);
    let cb = ctx.cur_block;
    ctx.f.set_term(
        cb,
        Terminator::CondBr {
            cond: is_null,
            then_blk: cold,
            else_blk: cell_test,
        },
    );
    ctx.cur_block = cell_test;
    let cell = ctx.f.append_inst(
        cell_test,
        InstKind::GlobalRef(UNDEF_CELL_SYM.to_string()),
        Type::Ptr,
        None,
    );
    let is_cell = ctx.cmp(IPred::Eq, v.clone(), Operand::Value(cell));
    ctx.f.set_term(
        cell_test,
        Terminator::CondBr {
            cond: is_cell,
            then_blk: cold,
            else_blk: live,
        },
    );
    ctx.cur_block = cold;
    emit_nullish_raise(ctx, raiser, v.clone());
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
/// [`emit_nullable_arr_guard`]: `str_null_check` arms a catchable
/// TypeError on any nullish repr, the throw-check right after
/// diverts before the inline `.length` load dereferences.
pub(crate) fn emit_nullable_str_guard(ctx: &mut LowerCtx<'_>, obj: ExprId, str_val: &Operand) {
    if !is_nullable_str_source(ctx, obj) && !is_undefable_substr_source(ctx, obj) {
        return;
    }
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Call(ctx.intrinsics.str_null_check, vec![str_val.clone()]),
    );
    ctx.emit_throw_check(None);
}
