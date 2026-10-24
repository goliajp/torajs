//! The rhs of a struct field store.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, StructId, Type};
use crate::ssa_lower::LowerCtx;

use super::fit;

/// What the rhs of a struct field store became.
pub(super) enum Rhs {
    /// A value for the slot, owned by the field once stored.
    Value(Operand),
    /// The any lane stored it already; this is the assignment's value.
    Stored(Operand),
}

/// Lower the rhs `value` of the store into `obj.field` (slot type
/// `field_ty`) and bring it into the slot's shape — or, for a value a
/// number slot cannot spell, store it through the any lane.
pub(super) fn store_rhs(
    ctx: &mut LowerCtx<'_>,
    (assign, obj): (ExprId, ExprId),
    obj_val: Operand,
    sid: StructId,
    field: &str,
    field_ty: Type,
    value: ExprId,
) -> Rhs {
    let v = if matches!(
        ctx.expr_types.get(&value),
        Some(crate::check::Type::Undefined)
    ) && let Some(sentinel) = ctx.str_undef_sentinel_for(field_ty)
    {
        // RFC 20260710-optional-undefined-repr C1 — an undefined
        // LITERAL into a Str/Substr slot stores the per-type
        // sentinel cell, not NULL (which means JS null). The cell
        // is immortal (FLAG_STATIC_LITERAL — rc/drop no-ops), so
        // no inc; the drop-old below stays correct for whatever
        // the slot held.
        sentinel
    } else if let Expr::Array(els) = ctx.ast.get_expr(value)
        && els.is_empty()
        && matches!(field_ty, Type::Arr(_))
    {
        let alloc_fn = if let Type::Arr(arr_id) = field_ty
            && ctx.arr_layouts[arr_id.0 as usize] == Type::Any
        {
            ctx.intrinsics.arr_alloc_any
        } else {
            ctx.intrinsics.arr_alloc
        };
        let cur_block = ctx.cur_block;
        let alloc = ctx.f.append_inst(
            cur_block,
            InstKind::Call(alloc_fn, vec![Operand::ConstI64(0)]),
            field_ty,
            None,
        );
        Operand::Value(alloc)
    } else if field_ty == Type::Any
        && let Some(w) =
            crate::ssa_lower_dstr_iter::try_lower_field_walk(ctx, value, &obj_val, sid, field)
    {
        // Rotation 455 — a generator-lifted destructure group temp
        // (`this.__dstra_src_N = init`, checker-recorded in
        // `iter_destr_srcs`): step the source through the iterator
        // protocol instead of storing it raw, so the index reads
        // below the lift land on a real Array<Any>. The walk's boxed
        // result is an OWNED stake the field takes verbatim — no
        // share inc (the general arm's `transfers` question is about
        // the SOURCE expression, which the walk already settled).
        w
    } else if field_ty == Type::Any && matches!(ctx.ast.get_expr(value), Expr::ObjectLit { .. }) {
        // Rotation 646 — an ObjectLit into a declared-Any field takes
        // the P3.2 dynobj lane the LetDecl path has
        // (`lower_let_init_val`). The plain lower minted a STRUCT and
        // the raw store boxed it, so `k.p = {}` on a class field — and
        // every `const p: any = {}` a generator lifts into its env —
        // carried a struct where `let p: any = {}` carries a dynobj:
        // `Object.defineProperty(p, 0, { get })` then `p[0]` read the
        // struct's expando lane, and the two spellings of one program
        // disagreed. The minted cell is an owned stake the field takes
        // verbatim (the any-literal array arm's shape).
        ctx.lower_dynobj_init(value)
    } else if let Expr::Array(els) = ctx.ast.get_expr(value)
        && let Type::Arr(arr_id) = field_ty
        && ctx.arr_layouts[arr_id.0 as usize] == Type::Any
    {
        // Chunk 614 — non-empty literal into an Any-elem field takes
        // the same annotation-consuming widen the LetDecl path has
        // (`lower_let_init_val`): without it the literal lowered
        // through the typed fast path, so the stored block never got
        // FLAG_ARR_ANY — the cycle collector's `is_visitable_arr`
        // said leaf, an `any[]` field cycle was invisible (obj root
        // buffered but the trial-deletion walk never crossed the
        // arr), and its raw scalar slots were NaN-box-misread by
        // every flag-dispatch consumer.
        let ids: Vec<ExprId> = els.clone();
        ctx.lower_array_any_literal(&ids)
    } else {
        // Chunk 784 — pin the field's declared struct layout for a
        // direct ObjectLit rhs (mirrors the chunk-780 let-decl site):
        // without it resolve_objlit_layout first-matches a
        // same-shaped layout registered under a different declared
        // type and the slot reprs collide (silent-wrong reads
        // through the declared layout).
        if ctx.pins_declared(field_ty, value) {
            let ann = ctx.member_field_ann(obj, field);
            ctx.pin_declared(field_ty, ann.as_deref(), value);
        }
        let v = ctx.lower_expr(value);
        ctx.let_declared_obj_layout = None;
        // a value a number slot cannot spell (an any-lane value, a
        // `null`) takes the any lane's [[Set]]: a number lands in the
        // slot, anything else in the field's sidecar
        let cannot_spell = crate::ssa_lower_object_lit::optional_number::number_slot_cannot_spell;
        if cannot_spell(ctx, field_ty, v, value) {
            let boxed = ctx.box_to_any(obj_val);
            let stored = crate::ssa_lower_assign_member_any::lower_dynobj_assign_value(
                ctx,
                assign,
                boxed,
                field,
                (value, v),
                &None,
                false,
            );
            return Rhs::Stored(stored);
        }
        // Chunk 566 — a field store SHARES the rhs (TS has no move
        // semantics): a borrow-shape value takes +1 so the field
        // owns its stake while the source binding keeps its own —
        // the old consume let a re-assign's drop-old steal the
        // source's only ref (UAF, reuse-window probe-proven). Owned
        // temps keep transferring their fresh reference — and so
        // does a fit that MINTED, whose value the source expression
        // never held (see `fit_to_field`).
        let (v, minted) = fit::fit_to_field(ctx, field_ty, v, value, (obj, field));
        let transfers = ctx.expr_transfers_ownership(value) || minted;
        let v_ty = ctx.operand_ty(&v);
        if !transfers && !v_ty.is_copy() {
            ctx.emit_rc_inc(v);
        }
        v
    };
    Rhs::Value(v)
}
