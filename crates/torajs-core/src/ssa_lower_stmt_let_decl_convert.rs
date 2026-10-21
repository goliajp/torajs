//! Type-boundary conversions for `let` init values, extracted from
//! [`crate::ssa_lower_stmt_let_decl`] (rotation 325 file-size split —
//! the As-share arm pushed the parent over 500). Two crossings and
//! their kind table:
//!
//! - `maybe_any_to_typed_scalar` — an `any` init under a `number` /
//!   `string` / `boolean` annotation decodes at the binding boundary.
//! - `maybe_arr_any_to_typed` — an `Array<Any>` init under a typed
//!   `Array<T>` annotation decode-copies into a fresh typed block
//!   (chunk 698). Also serves the return-boundary caller in
//!   `ssa_lower_stmt_return` and the global lane.

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// An `any` init bound to a `number` / `string` / `boolean`
/// annotation: §7.1.4 ToNumber via `any_to_number` / §7.1.17 ToString
/// via `coerce_to_str` / §7.1.2 ToBoolean via `coerce_to_bool`, so
/// the slot holds what the box carried rather than the box's bits.
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
/// A read of an `any` value or an `as any` cast never gets here: the
/// binding holds the box itself (`check_stmt_let_decl::any_scalar_slot`).
/// What still crosses is a call's result under a lying annotation
/// (`const x: number = g()`, `g` answering `"hi"`), which answers NaN
/// — ToNumber, where bun keeps the string.
pub(crate) fn maybe_any_to_typed_scalar(
    ctx: &mut LowerCtx,
    type_ann: Option<&String>,
    ty: Type,
    init: ExprId,
    init_val: Operand,
) -> (Operand, bool) {
    if type_ann.is_none() {
        return (init_val, false);
    }
    // A heap value reaching a scalar slot: it gets here through `as
    // any`, which lowers to the value's OWN representation while the
    // checker admits Any into the slot — so the operand's static type
    // is the heap one and none of the Any arms below ever see it. The
    // slot then read the POINTER as its scalar: `let s: string = ([4]
    // as any)` answered "\u0001". Boxing is an rc-neutral encode of
    // the init's own reference (the same step the argument position
    // has always taken), so `release_owned_temp` below still settles
    // the original.
    let source = init_val;
    let init_val =
        if crate::ssa_lower_call_arg_conv::heap_into_primitive(ty, ctx.operand_ty(&init_val)) {
            ctx.box_to_any(init_val)
        } else {
            init_val
        };
    if ctx.operand_ty(&init_val) != Type::Any {
        return (init_val, false);
    }
    use crate::ssa_lower_checked_cast::{Admits, CastScope, CastSlot};
    // `string | undefined` is storage-identical to `string` by the time
    // `ty` exists, so the annotation text is the only place left that
    // knows the union admits undefined. Without this the cast rejects
    // the union's own member and a legal program becomes a TypeError.
    let admits = match type_ann {
        Some(t) if t.starts_with("__nullable(") => Admits::OrUndefined,
        _ => Admits::Exact,
    };
    let cast = ctx.checked_cast(init_val, ty, CastSlot::Assign, CastScope::Annotated, admits);
    let decoded = match ty {
        _ if cast.is_some() => cast.expect("guard just tested it"),
        Type::I64 | Type::F64 => ctx.coerce_any_to_number(init_val, ty),
        // A declared `string` slot, not a ToString site: `undefined`
        // keeps the Str undefined sentinel. Running the real §7.1.17
        // here spelled it as the six-letter TEXT "undefined" (and
        // `null` as "null"), so `let s: string | undefined = (undefined
        // as any)` answered a 9-character string — `s === undefined`
        // false, `typeof s` "string", where bun says undefined. The
        // sentinel is what every other boundary into a Str slot
        // already uses: the boxed param adapter
        // (`__torajs_anyv_to_str_param`, the same kernel), a missing
        // argument, and `str_undef_sentinel_for`'s literal path.
        Type::Str => {
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.any_to_str_slot, vec![init_val]),
                Type::Str,
                None,
            );
            Operand::Value(v)
        }
        // §7.1.2 ToBoolean via coerce_to_bool's Any arm. Without this
        // row a `boolean` binding took the box's raw BITS (a `false`
        // box is a non-zero pattern — every any-held value read as
        // true, the L3b ① TypedNarrow-lane discovery).
        Type::Bool => ctx.coerce_to_bool(init_val),
        // An object layout the annotation names takes the value only as
        // the cell it is (`ssa_lower_checked_cast::obj`).
        Type::Obj(sid) => {
            let ann = type_ann.expect("an unannotated binding returned above");
            match ctx.checked_cast_obj(init_val, sid, ann, CastSlot::Assign) {
                Some(v) => v,
                None => return (init_val, false),
            }
        }
        // an array of object layouts takes each element as the element
        // slot would
        Type::Arr(_) => {
            let ann = type_ann.expect("an unannotated binding returned above");
            match ctx.checked_cast_obj_arr(init_val, ty, ann, CastSlot::Assign) {
                Some(v) => v,
                None => return (init_val, false),
            }
        }
        _ => return (init_val, false),
    };
    // The original reference, not the box: boxing above re-encoded it
    // rather than taking a stake of its own.
    ctx.release_owned_temp(init, &source);
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
    if init_ty == Type::Any {
        return any_box_to_typed_arr(ctx, ty, init, init_val);
    }
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

/// An owned `any` value (`xs.map(anyCb)`, whose kernel answers an
/// `Array<Any>` block behind an `any` type) under a typed `Array<T>`
/// annotation or return type: the same decode-copy through
/// `__torajs_arr_from_any_typed`. Taking the box's bits as the typed
/// block read every slot as a NaN-box (0.5 printed 1.4375).
fn any_box_to_typed_arr(
    ctx: &mut LowerCtx,
    ty: Type,
    init: ExprId,
    init_val: Operand,
) -> (Operand, bool) {
    let Some(kind) = crate::ssa_lower_assign_slot_fit::arr_slot_takes_any(ctx, ty, Type::Any)
    else {
        return (init_val, false);
    };
    let fid = *ctx
        .fn_table
        .get("__torajs_arr_from_any_typed")
        .expect("__torajs_arr_from_any_typed intrinsic missing");
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(fid, vec![init_val, Operand::ConstI64(kind)]),
        ty,
        None,
    );
    ctx.emit_throw_check(None);
    ctx.release_owned_temp(init, &init_val);
    (Operand::Value(v), true)
}

/// Target elem type → `ARR_KIND_*` constant for the chunk-698
/// assign-boundary conversion (mirror of `arr_kind_chain`'s low
/// bits). `None` = no runtime kind can express the elem — the decl
/// keeps the pre-698 behavior (no conversion).
pub(crate) fn arr_elem_kind_const(elem: &Type) -> Option<i64> {
    Some(match elem {
        Type::I64 | Type::I32 => 1,
        Type::F64 => 2,
        Type::Bool => 3,
        t if t.is_refcounted() => 4,
        _ => return None,
    })
}
