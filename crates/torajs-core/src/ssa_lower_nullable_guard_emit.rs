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

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand};
use crate::ssa_lower::LowerCtx;
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
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Call(ctx.intrinsics.arr_null_check, vec![arr_val.clone()]),
    );
    ctx.emit_throw_check(None);
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
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Call(ctx.intrinsics.heap_nullish_check, vec![obj_val.clone()]),
    );
    ctx.emit_throw_check(None);
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
