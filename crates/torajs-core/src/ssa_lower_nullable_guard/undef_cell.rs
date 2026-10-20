//! The pointer slot holding the generic immortal cell: whether a value
//! may be it, and the surfaces that must not read it as a live value.

use crate::ast::ExprId;
use crate::ssa_lower::LowerCtx;

use super::is_undefable_heap_source;

/// `true` when this expression's value may be the generic immortal
/// cell right now — the in-band spelling of `undefined` for a
/// pointer-shaped slot.
///
/// It is the pair of questions every consumer of such a slot has to
/// ask, and asking only one of them is wrong in both directions: the
/// TYPE says whether this slot family has a cell at all, and the
/// SOURCE says whether this particular value could have come from a
/// road that hands one out. It had eight hand-written copies
/// before it had a name, two of them inside the any boundary
/// itself with the halves in the other order — which is seven too
/// many for a question with one answer, and is how a consumer
/// ends up asking half of it.
///
/// Consumers that answer `true` here should hand the value to the
/// any boundary (`box_to_any_from_expr`), which re-encodes the cell
/// as ANY_UNDEF, rather than reading it as a live value of its
/// static type.
pub(crate) fn holds_undef_cell(ctx: &LowerCtx<'_>, eid: ExprId, ty: crate::ssa::Type) -> bool {
    ty.spells_undef_with_generic_cell() && is_undefable_heap_source(ctx, eid)
}

/// The argument of a ToObject surface (`Object.{keys,values,
/// entries}` and their `Reflect` / `getOwnPropertyNames` aliases),
/// paired with the type the rest of the lowering should dispatch
/// on.
///
/// §7.1.18 throws a TypeError for undefined, and a pointer-shaped
/// slot is holding undefined whenever it holds the immortal cell.
/// The typed arms downstream read it as a live value of their own
/// type instead: the Arr arm loads a length off the cell, and the
/// struct arm walks the compile-time layout — which is how
/// `Object.values(o)` came back as `[8752868413093667698]`,
/// whatever the field offset landed on, while `Object.keys(o)`
/// answered `["a"]` for a value with no properties at all.
///
/// The any lane already answers §7.1.18 for all three surfaces
/// (probed: each throws TypeError), so such an argument goes
/// through the boundary that re-encodes the cell and takes that
/// lane. This has to happen before the Arr arm rather than at the
/// later `to_object_needs_boxing` point, which those arms return
/// in front of.
pub(crate) fn to_object_arg(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    op: crate::ssa::Operand,
) -> (crate::ssa::Operand, crate::ssa::Type) {
    let ty = ctx.operand_ty(&op);
    if holds_undef_cell(ctx, eid, ty) {
        return (ctx.box_to_any_from_expr(eid, op), crate::ssa::Type::Any);
    }
    (op, ty)
}

/// The type that carries a read of a `T` element when that read has
/// an exit answering `undefined` — an out-of-range index, `at`, a
/// `find` miss, `pop` / `shift` on an empty array.
///
/// Every type answers in its own slot, because each has a bit
/// pattern to spare that no live value uses: the F64 sentinel NaN,
/// the Str / Substr oddballs, the generic cell for pointers. `Bool`
/// is the one that does not — a bool is exactly two states — so its
/// read comes back as a tagged value instead. Only the value handed
/// back changes; the array's own slots stay bool-shaped, so nothing
/// is paid by an array that is never read past its end.
///
/// Promoting rather than inventing a third bool state is what keeps
/// this right for consumers nobody has written yet: an in-band third
/// state has to be remembered at every consumer forever, and the two
/// conversions that mask a bool to its low bit would erase it into
/// `false` on the way through.
pub(crate) fn undefable_read_ty(elem_ty: crate::ssa::Type) -> crate::ssa::Type {
    if elem_ty == crate::ssa::Type::Bool {
        crate::ssa::Type::Any
    } else {
        elem_ty
    }
}
