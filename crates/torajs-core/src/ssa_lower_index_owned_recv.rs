//! The typed `Arr<T>` lanes of an indexed read, and the accounting an
//! owned-temp receiver needs around them — split from
//! `ssa_lower_index` at its size limit.

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// The element of a typed `Arr<T>` read: the keyed kernel for a
/// non-numeric key domain, the inline `Array<Any>` read, or the
/// bounds-checked typed load.
pub(crate) fn lower_typed_arr(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    obj: ExprId,
    index: ExprId,
    arr_val: Operand,
    arr_ty: Type,
    is_non_deque: bool,
) -> Operand {
    if matches!(arr_ty, Type::Arr(_))
        && ctx
            .expr_types
            .get(&index)
            .is_some_and(crate::check_type_of_index::array_key_domain)
    {
        return crate::ssa_lower_index::lower_typed_arr_any_key(ctx, eid, arr_val, index);
    }
    // RC-4 F1a — a nullable-arr receiver (exec/match result) may be
    // null on miss; guard before the element load (both Arr<Any>
    // and typed Arr<T> lanes).
    crate::ssa_lower_nullable_guard::emit_nullable_arr_guard(ctx, obj, &arr_val);
    let elem_ty = match arr_ty {
        Type::Arr(arr_id) => ctx.arr_layouts[arr_id.0 as usize],
        // Same as the member lane: the receiver is a read that already
        // RAISED, so this element load is unreachable and `arr_val` is
        // only the raise's stand-in shape
        // (`ssa_lower_ident::read_raises`).
        _ if crate::ssa_lower_ident::read_raises(ctx.ast, obj) => {
            return Operand::ConstPtrNull;
        }
        other => panic!("ssa-lower: index access on non-array type {other:?}"),
    };
    let (idx_val, nonint) = crate::ssa_lower_index_nonintegral::lower_typed_read_index(ctx, index);
    if elem_ty == Type::Any {
        // An index that is not an integer names a property, not an
        // element — the keyed kernel answers it (an expando, or
        // undefined), the way an `any` receiver already does.
        if let Some(split) = nonint {
            ctx.owned_member_reads.insert(eid);
            return crate::ssa_lower_index_nonintegral::emit_any_elem_read_split(
                ctx, arr_val, idx_val, split,
            );
        }
        return crate::ssa_lower_index::lower_array_any_index(ctx, arr_val, idx_val);
    }
    // Guard-dominated bounds elision — an enclosing `i < xs.length`
    // guard settles the upper compare, so it and the length load it
    // needs (the aliasing wall LLVM cannot hoist past the raw-pointer
    // LoadDyn) drop out. See [`crate::ssa_lower_bounds_proven`].
    let upper_proven = crate::ssa_lower_bounds_proven::is_proven(ctx, eid);
    // The receiver of a field read: the exits that would answer
    // `undefined` raise the TypeError that read would raise on it,
    // and the element flows on without a merge.
    if let Type::Obj(sid) | Type::Inline(sid) = elem_ty
        && nonint.is_none()
        && ctx.proofs.recv_field.get(&eid).is_some_and(|field| {
            ctx.struct_layouts[sid.0 as usize]
                .iter()
                .any(|(k, _)| k == field.as_str())
        })
    {
        ctx.proofs.nonnull_exprs.insert(eid);
        return crate::ssa_lower_index_undef::recv::lower_typed_index_receiver(
            ctx,
            arr_val,
            idx_val,
            is_non_deque,
            elem_ty,
            upper_proven,
        );
    }
    // A read that can reach a getter answers an owned value through
    // that exit (`emit_nonintegral_typed_read`), so the whole read is
    // owned: a `boolean[]` element is an immediate whose release is
    // nothing, and the `string[]` lane takes a stake on its in-range
    // element (`lower_typed_index_checked`).
    if matches!(elem_ty, Type::Bool | Type::Str)
        && (nonint.is_some() || ctx.ast.array_prop_reads_may_raise)
    {
        ctx.owned_member_reads.insert(eid);
    }
    crate::ssa_lower_index::lower_typed_index_checked(
        ctx,
        arr_val,
        idx_val,
        is_non_deque,
        elem_ty,
        upper_proven,
        nonint,
        ctx.ast.array_prop_reads_may_raise,
    )
}

/// When the receiver is an owned temp, the element the lane answered
/// as a borrow off the receiver's slot takes its own stake — a view
/// inline in a split block through `substr_share`, since the block
/// is about to go — and is recorded as an owned read so its consumer
/// releases it; then the receiver is released. A lane that already
/// answered an owned cell (the keyed kernel's +1) only loses the
/// receiver. Copy elements need nothing. `xs[i]` on a named receiver
/// emits nothing here.
pub(crate) fn detach_from_owned_receiver(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    obj: ExprId,
    arr_val: &Operand,
    elem: Operand,
) -> Operand {
    if !ctx.expr_owned_shape(obj) {
        return elem;
    }
    let vt = ctx.operand_ty(&elem);
    let elem = if vt.is_refcounted() && !ctx.owned_member_reads.contains(&eid) {
        ctx.owned_member_reads.insert(eid);
        if vt == Type::Substr {
            let cur = ctx.cur_block;
            Operand::Value(ctx.f.append_inst(
                cur,
                InstKind::Call(ctx.intrinsics.substr_share, vec![elem]),
                Type::Substr,
                None,
            ))
        } else {
            ctx.emit_owned_result_inc(elem, vt);
            elem
        }
    } else {
        elem
    };
    ctx.release_owned_temp(obj, arr_val);
    elem
}
