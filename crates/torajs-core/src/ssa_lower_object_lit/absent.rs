//! Fields a literal leaves out of the type it is stored as.
//!
//! An optional field the literal does not spell (`const p: { a?:
//! number; b: number } = { b: 2 }`) is not an own property of the
//! object (§10.1.11): `"a" in p` is false and `Object.keys(p)` lists
//! only `b`. The checker records such a literal with the declared field
//! list, so the cell takes the declared layout and every typed read of
//! `p.a` stays one load. The slot holds the field type's undefined, and
//! the field is recorded deleted on the cell, the same record a
//! `delete` of a declared field leaves: the own faces skip it, and the
//! first store to the field creates the property again.

use crate::ast::{ExprId, PropKey};
use crate::ssa::{IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::LowerCtx;

/// The declared fields the literal at `eid` does not spell, in
/// declared order with their declared index — empty unless the checker
/// recorded the literal with more fields than it has.
pub(super) fn absent_fields(
    ctx: &LowerCtx<'_>,
    eid: ExprId,
    entries: &[(PropKey, ExprId)],
) -> Vec<(usize, PropKey)> {
    match ctx.expr_types.get(&eid) {
        Some(t) => crate::check_assignable::literal_absent_fields(t, entries),
        None => Vec::new(),
    }
}

/// Put each absent field in at its declared position as the operand an
/// `undefined` literal lowers to; the caller re-materializes it as the
/// slot's undefined once the layout is resolved.
pub(super) fn insert_absent(
    absent: &[(usize, PropKey)],
    field_tys: &mut Vec<(PropKey, Type)>,
    field_vals: &mut Vec<Operand>,
) {
    for (i, name) in absent {
        let at = (*i).min(field_tys.len());
        field_tys.insert(at, (name.clone(), Type::Ptr));
        field_vals.insert(at, Operand::ConstPtrNull);
    }
}

/// An Any slot's undefined (`ANY_UNDEF`, payload 0).
pub(crate) fn any_undef(ctx: &mut LowerCtx<'_>) -> Operand {
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::BoxAny(5, Operand::ConstI64(0)),
        Type::Any,
        None,
    );
    Operand::Value(v)
}

/// The own-property answer for the declared field `key` of the struct
/// `obj`, which the layout lists: true, unless some literal leaves a
/// field of that name out, when the cell's own record decides.
pub(crate) fn declared_field_own(
    ctx: &mut LowerCtx<'_>,
    obj: Operand,
    key: &torajs_wtf8::Wtf8,
) -> Operand {
    if !ctx.ast.absent_field_names.contains(key) {
        return Operand::ConstBool(true);
    }
    let deleted = field_deleted(ctx, obj, key);
    let own = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Eq, deleted, Operand::ConstI64(0)),
        Type::Bool,
        None,
    );
    Operand::Value(own)
}

/// 1 when the declared field `key` of the struct `obj` is recorded
/// deleted, else 0 (I64).
pub(crate) fn field_deleted(
    ctx: &mut LowerCtx<'_>,
    obj: Operand,
    key: &torajs_wtf8::Wtf8,
) -> Operand {
    let k = ctx.intern_string_literal(key);
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.obj_field_deleted,
            vec![obj, Operand::Value(k)],
        ),
        Type::I64,
        None,
    );
    Operand::Value(v)
}

/// Record each field deleted on the finished cell whose flag (I64,
/// nonzero = every spread source that contributed it lacked it) is set.
pub(super) fn mark_absent_if(ctx: &mut LowerCtx<'_>, obj: ValueId, absent: &[(PropKey, Operand)]) {
    for (name, flag) in absent {
        let hit = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::ICmp(IPred::Ne, *flag, Operand::ConstI64(0)),
            Type::Bool,
            None,
        );
        let del_blk = ctx.f.add_block();
        let cont = ctx.f.add_block();
        ctx.f.set_term(
            ctx.cur_block,
            Terminator::CondBr {
                cond: Operand::Value(hit),
                then_blk: del_blk,
                else_blk: cont,
            },
        );
        ctx.cur_block = del_blk;
        mark_absent(ctx, obj, &[(0, name.clone())]);
        let end = ctx.cur_block;
        ctx.f.set_term(end, Terminator::Br(cont));
        ctx.cur_block = cont;
    }
}

/// Record each absent field deleted on the finished cell.
pub(super) fn mark_absent(ctx: &mut LowerCtx<'_>, obj: ValueId, absent: &[(usize, PropKey)]) {
    if absent.is_empty() {
        return;
    }
    let recv = ctx.box_to_any(Operand::Value(obj));
    for (_, name) in absent {
        let key = ctx.intern_string_literal(name);
        ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.any_prop_delete_soft,
                vec![recv, Operand::Value(key)],
            ),
            Type::I64,
            None,
        );
    }
}
