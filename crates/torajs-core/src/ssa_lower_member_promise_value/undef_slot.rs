//! The settled value of a `Promise<T | undefined>` whose `T` is
//! pointer-shaped (a string, an object, an array): its slot spells
//! `undefined` with the slot type's own immortal cell and `null` with
//! NULL, but the runtime's conversion into a pointer form answers NULL
//! for both, so an undefined result read back as `null`. The cell's
//! stamp still tells them apart; the read asks for it as a box and, on
//! `undefined`, answers the slot's cell instead.

use crate::ast::ExprId;
use crate::check as check_mod;
use crate::ssa::{IPred, InstKind, Operand, Type, ValueId};
use crate::ssa_lower::LowerCtx;

/// The slot type whose undefined cell a read of `obj`'s settled value
/// must answer, when the checker typed `obj` a promise of a nullable
/// that the read lane `inner` holds raw (a scalar nullable reads in the
/// any lane, which keeps `undefined` on its own).
pub(crate) fn undef_cell_slot(
    ctx: &LowerCtx<'_>,
    obj: ExprId,
    inner: Option<&Type>,
) -> Option<Type> {
    let Some(check_mod::Type::Promise(t)) = ctx.expr_types.get(&obj) else {
        return None;
    };
    if !matches!(**t, check_mod::Type::Nullable(_)) {
        return None;
    }
    let ty = *inner?;
    crate::ssa_lower_binop_null_undef::undef_cell_sym(ty).map(|_| ty)
}

/// `raw` (the settled value word read in `slot`'s form), with an
/// `undefined` result answered as `slot`'s undefined cell. `cell` is
/// the settled promise, already past its rejection check.
pub(crate) fn keep_undefined(
    ctx: &mut LowerCtx<'_>,
    cell: Operand,
    raw: ValueId,
    slot: Type,
) -> ValueId {
    let cb = ctx.cur_block;
    let any = Operand::ConstI64(crate::ssa_lower_promise_repr_mark::REPR_ANY);
    let read = InstKind::Call(ctx.intrinsics.promise_get_value_as, vec![cell, any]);
    let boxed = ctx.f.append_inst(cb, read, Type::I64, None);
    let undef = torajs_anyvalue::nanbox::VALUE_UNDEFINED as i64;
    let is_undef = ctx.cmp(IPred::Eq, Operand::Value(boxed), Operand::ConstI64(undef));
    let sentinel = ctx
        .str_undef_sentinel_for(slot)
        .expect("undef_cell_slot answered a slot with an undefined cell");
    let cb = ctx.cur_block;
    let word = ctx
        .f
        .append_inst(cb, InstKind::PtrToInt(sentinel), Type::I64, None);
    let picked = ctx.select_i64(
        is_undef,
        Operand::Value(word),
        Operand::Value(raw),
        "__promise_undef_word",
    );
    let Operand::Value(v) = picked else {
        unreachable!("select_i64 answers the slot's load")
    };
    v
}
