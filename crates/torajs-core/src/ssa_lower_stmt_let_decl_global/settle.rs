//! Ownership at a top-level global's init store. A global outlives
//! every binding it could borrow from, so it never borrows: whatever
//! the init's shape, the slot leaves the store holding one stake of
//! its own, which the exit drop releases. [`settle_any_read_init`]
//! answers an `any` slot fed an `any` read, [`settle_typed_init`] a
//! typed refcounted slot.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// An `any` slot whose init is already an `any` READ — a member /
/// index load or an optional chain over one, or the default join over
/// one (`let a = __ary_src_0[0]`,
/// the destructuring bind `ast_refs_any_promote` admits). The fn-scope
/// lane binds such a read as a borrow (`init_is_alias`) or takes a share
/// of a join (`init_shares_source_stake`); a global has no borrowed
/// mode, it outlives every source, so it takes its own stake in both
/// cases. A read the lowering detached (`owned_member_reads`) arrives
/// owned and transfers, as does a call's result. An optional chain's
/// hit arm may box a borrowed element (the typed-array arm stays off
/// the owned track), so it settles like any other read. `true` = ownership is settled here, which also
/// keeps these shapes off [`settle_typed_init`].
pub(super) fn settle_any_read_init(
    ctx: &mut LowerCtx,
    init: ExprId,
    slot_ty: Type,
    init_val: &Operand,
) -> bool {
    if slot_ty != Type::Any || ctx.operand_ty(init_val) != Type::Any {
        return false;
    }
    let read = matches!(
        ctx.ast.get_expr(init),
        Expr::Member { .. } | Expr::Index { .. } | Expr::OptIndex { .. } | Expr::OptChain { .. }
    );
    let join = matches!(
        ctx.ast.get_expr(init),
        Expr::Ternary { .. } | Expr::Nullish { .. }
    );
    if !(read || join) {
        return false;
    }
    if !ctx.owned_member_reads.contains(&init) && !ctx.expr_transfers_ownership(init) {
        ctx.emit_owned_result_inc(*init_val, Type::Any);
    }
    read
}

/// A typed refcounted slot. A string view (`s[0]`, a sliced string)
/// cannot live in a `string` slot, whose readers decode owned cells,
/// so it is copied out first. Past that, the ledger's two borrow
/// shapes take the slot's own stake: an alias of another binding, and
/// a member or element read the lowering did not detach (the rows the
/// fn-scope lane binds as a borrow, `init_is_alias`). Every other
/// shape hands over a fresh value.
pub(super) fn settle_typed_init(
    ctx: &mut LowerCtx,
    name: &str,
    init: ExprId,
    slot_ty: Type,
    init_val: Operand,
) -> Operand {
    if !slot_ty.is_refcounted() {
        return init_val;
    }
    let got = ctx.operand_ty(&init_val);
    if slot_ty == Type::Str && got == Type::Substr {
        return substr_into_str(ctx, init, init_val);
    }
    match ctx.ast.get_expr(init) {
        Expr::Ident(_) => {
            if !ident_alias_fits(ctx, name, slot_ty, got) {
                panic!(
                    "ssa-lower: K.4 refcount global `{name}` requires a same-type alias init; `{got:?}` into `{slot_ty:?}` is not yet supported"
                );
            }
            ctx.emit_owned_result_inc(init_val, got);
        }
        Expr::Member { .. } | Expr::Index { .. } => {
            if got != slot_ty && !variadic_cell_fits(ctx, name, slot_ty, got) {
                panic!(
                    "ssa-lower: K.4 refcount global `{name}` reads a `{got:?}` into a `{slot_ty:?}` slot; this init shape is not yet supported"
                );
            }
            if !ctx.owned_member_reads.contains(&init) && !ctx.expr_transfers_ownership(init) {
                ctx.emit_owned_result_inc(init_val, got);
            }
        }
        _ => {}
    }
    init_val
}

/// RFC 20260707 chunk 627 — an Ident-shaped init (alias of another
/// binding, a guaranteed +0 borrow). Only same-type, Arr↔Arr (T-11
/// container widen) and Any→Arr (chunk 708 — the JS reference-alias
/// shape `const t: number[] = src` with `src: any`; a heap box is the
/// raw cell ptr bits, elem reads are kind-aware, same lane the fn-scope
/// path rides) aliases are admitted — other Any↔concrete mismatches
/// need a box/unbox station and stay loud rather than storing
/// wrong-repr bits.
///
/// A VARIADIC binding's two Closure spellings are the same env cell:
/// the slot carries only the fixed prefix (a rest gets no static param
/// slot — `parse_cls` drops the `__rest(` segment), while the source
/// binding holds the lifted fn's own sig, which spells that rest tail
/// as its one array parameter. Neither is read by a call through this
/// binding — the route is the boxed dual entry, which is exactly what
/// `variadic_value_bindings` names — so the sig ids are free to differ
/// and the bits stored are identical either way.
fn ident_alias_fits(ctx: &LowerCtx, name: &str, slot_ty: Type, got: Type) -> bool {
    got == slot_ty
        || matches!(
            (&slot_ty, &got),
            (Type::Arr(_), Type::Arr(_)) | (Type::Arr(_), Type::Any)
        )
        || variadic_cell_fits(ctx, name, slot_ty, got)
}

/// The variadic-binding case of [`ident_alias_fits`], shared with a
/// read of a variadic closure field (`const g = src.f`): the field
/// holds the env cell under the lifted fn's own sig, the slot under
/// the fixed prefix, and the call route never reads either.
fn variadic_cell_fits(ctx: &LowerCtx, name: &str, slot_ty: Type, got: Type) -> bool {
    matches!((&slot_ty, &got), (Type::Closure(_), Type::Closure(_)))
        && ctx.ast.variadic_value_bindings.contains(name)
}

/// Copy a string view out into an owned string for a `string` slot;
/// a view the init owned (string indexing, a call's result) is
/// released, one it borrows (an alias of a view binding) is left to
/// its owner.
fn substr_into_str(ctx: &mut LowerCtx, init: ExprId, view: Operand) -> Operand {
    let cur_block = ctx.cur_block;
    let owned = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.substr_to_owned, vec![view]),
        Type::Str,
        None,
    );
    if ctx.owned_member_reads.contains(&init) || ctx.expr_transfers_ownership(init) {
        ctx.emit_drop_value(view, Type::Substr);
    }
    Operand::Value(owned)
}

/// An `any` init reaching a scalar slot. An annotated binding decodes
/// as the fn-scope lane does (`maybe_any_to_typed_scalar`). An
/// un-annotated one took its slot from the init's shape
/// (`ast_refs::infer_toplevel_slot_shape`), which answers a number for
/// `x >>> 0` or `x & 1` whatever `x` holds, while the operator's
/// kernel over an `any` operand answers a box: §7.1.4 ToNumber reads
/// the number back out of it rather than storing the box's bits.
pub(super) fn any_into_scalar(
    ctx: &mut LowerCtx,
    type_ann: Option<&String>,
    slot_ty: Type,
    init: ExprId,
    init_val: Operand,
) -> (Operand, bool) {
    if type_ann.is_some()
        || !matches!(slot_ty, Type::I64 | Type::F64)
        || ctx.operand_ty(&init_val) != Type::Any
    {
        return crate::ssa_lower_stmt_let_decl::maybe_any_to_typed_scalar(
            ctx, type_ann, slot_ty, init, init_val,
        );
    }
    let decoded = ctx.coerce_any_to_number(init_val, slot_ty);
    ctx.release_owned_temp(init, &init_val);
    (decoded, true)
}
