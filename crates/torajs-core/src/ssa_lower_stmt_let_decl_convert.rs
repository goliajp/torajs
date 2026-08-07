//! Type-boundary conversions for `let` init values, extracted from
//! [`crate::ssa_lower_stmt_let_decl`] (rotation 325 file-size split —
//! the As-share arm pushed the parent over 500). Two crossings and
//! their kind table:
//!
//! - `maybe_any_to_typed_scalar` — an `any` init under a `number` /
//!   `string` annotation decodes at the binding boundary.
//! - `maybe_arr_any_to_typed` — an `Array<Any>` init under a typed
//!   `Array<T>` annotation decode-copies into a fresh typed block
//!   (chunk 698). Also serves the return-boundary caller in
//!   `ssa_lower_stmt_return` and the global lane.

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// An `any` init bound to a `number` or `string` annotation: §7.1.4
/// ToNumber via `any_to_number` / §7.1.17 ToString via `coerce_to_str`,
/// so the slot holds what the box carried rather than the box's bits.
/// Answers `(value, converted)`; a `true` flag joins the chunk-698
/// `converted` above — either way the binding owns its result outright
/// and neither aliases nor shares the source.
///
/// Only an ANNOTATED binding crosses: without an annotation the slot
/// type follows the init (`finalize_and_bind` re-reads `operand_ty`),
/// so `any` stays `any` and there is nothing to decode.
///
/// Ownership differs by row, and both ends are settled here:
/// `any_to_number` only READS its argument (chunk 566) and leaves a
/// Copy slot behind, while `coerce_to_str` mints a fresh-owned Str the
/// binding then drops at scope close. Either way an owned Any temp (a
/// call result, a fresh box) still carries a stake the new slot won't
/// answer for — `release_owned_temp` settles it, and leaves borrow
/// shapes (Ident / Member / Index) untouched since their source keeps
/// its own.
///
/// A lying annotation (`const x: number = t`, `t` holding `"hi"`)
/// answers NaN. That is ToNumber, not type erasure — bun keeps the
/// string, since TS annotations vanish at runtime. Both sibling
/// boundaries answer NaN the same way, so these rows join the existing
/// stance rather than inventing a third one; the erasure gap itself is
/// one L3b entry against all three.
pub(crate) fn maybe_any_to_typed_scalar(
    ctx: &mut LowerCtx,
    type_ann: Option<&String>,
    ty: Type,
    init: ExprId,
    init_val: Operand,
) -> (Operand, bool) {
    if type_ann.is_none() || ctx.operand_ty(&init_val) != Type::Any {
        return (init_val, false);
    }
    let decoded = match ty {
        Type::I64 | Type::F64 => ctx.coerce_any_to_number(init_val.clone(), ty),
        Type::Str => ctx.coerce_to_str(init_val.clone(), Type::Any),
        _ => return (init_val, false),
    };
    ctx.release_owned_temp(init, &init_val);
    (decoded, true)
}

/// Chunk 698 — `Array<Any>` init bound to a typed `Array<T>`
/// annotation: decode-copy into a fresh typed block via
/// `__torajs_arr_any_to_typed` (mismatch = catchable TypeError).
/// Answers `(value, converted)`; a `false` flag keeps the caller's
/// pre-698 bookkeeping. Gated on the init's Any elem + a mappable
/// target kind so typed→typed decls pay no call.
pub(crate) fn maybe_arr_any_to_typed(
    ctx: &mut LowerCtx,
    ty: Type,
    init: ExprId,
    init_val: Operand,
) -> (Operand, bool) {
    // Owned shapes only (Call / literal / BinOp): the fresh value
    // has no other reference, so the decode-copy is unobservable.
    // A borrow-shape init (`const t: number[] = src`) is a JS
    // reference alias — copying it would detach `t` from `src`'s
    // later mutations; those keep the pre-698 behavior (L3b).
    if !ctx.expr_owned_shape(init) {
        return (init_val, false);
    }
    let init_ty = ctx.operand_ty(&init_val);
    let (Type::Arr(ann_id), Type::Arr(init_id)) = (&ty, &init_ty) else {
        return (init_val, false);
    };
    if ctx.arr_layouts[init_id.0 as usize] != Type::Any
        || ctx.arr_layouts[ann_id.0 as usize] == Type::Any
    {
        return (init_val, false);
    }
    let Some(kind) = arr_elem_kind_const(&ctx.arr_layouts[ann_id.0 as usize]) else {
        return (init_val, false);
    };
    let fid = *ctx
        .fn_table
        .get("__torajs_arr_any_to_typed")
        .expect("__torajs_arr_any_to_typed intrinsic missing");
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(fid, vec![init_val, Operand::ConstI64(kind)]),
        ty,
        None,
    );
    ctx.emit_throw_check(None);
    // A fresh owned init (Call / literal) hands its only ref off
    // here; a borrowed shape (Ident / Member) keeps the source
    // binding's stake untouched (the helper always copies).
    ctx.release_owned_temp(init, &init_val);
    (Operand::Value(v), true)
}

/// Target elem type → `ARR_KIND_*` constant for the chunk-698
/// assign-boundary conversion (mirror of `arr_kind_chain`'s low
/// bits). `None` = no runtime kind can express the elem — the decl
/// keeps the pre-698 behavior (no conversion).
fn arr_elem_kind_const(elem: &Type) -> Option<i64> {
    Some(match elem {
        Type::I64 | Type::I32 => 1,
        Type::F64 => 2,
        Type::Bool => 3,
        t if t.is_refcounted() => 4,
        _ => return None,
    })
}
