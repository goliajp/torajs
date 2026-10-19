//! Which checker types land in a slot that can hold the generic
//! immortal cell.

/// The checker-side reading of [`crate::ssa::Type::
/// spells_undef_with_generic_cell`] — true when a value of this type
/// lands in a slot that can hold the generic immortal cell.
///
/// The two are mirrors and must move together: a checker type that
/// starts lowering to a refcounted pointer belongs on the `true` side
/// here the same day. Both are exhaustive so that adding a type is a
/// build error rather than a silent fall into the wrong half — the
/// `container_key_lookup` mirror taught that lesson by answering the
/// wrong class for a name it had never heard of.
///
/// One-way slack is fine and deliberate: saying `true` for something
/// that turns out to lower to `FnSig` only buys a well-predicted
/// identity compare, because the emitting sites gate on the SSA type
/// as well. Saying `false` for something that does hold the cell is
/// the direction that goes silently wrong, so `Nullable` recurses
/// rather than guessing.
pub(crate) fn spells_undef_with_generic_cell(t: &crate::check::Type) -> bool {
    use crate::check::Type as T;
    match t {
        T::Struct(_)
        | T::ClassRef(_)
        | T::Array(_)
        | T::Function(..)
        | T::BigInt
        | T::Date
        | T::RegExp
        | T::Symbol
        | T::Promise(_)
        | T::Map(..)
        | T::Set
        | T::MapIter(..)
        | T::ArrIter
        | T::WeakRef
        | T::WeakMap
        | T::WeakSet => true,
        // `T | null` for a pointer-shaped T is that slot with NULL in band
        T::Nullable(inner) => spells_undef_with_generic_cell(inner),
        // a union and a literal type lower as their lane
        T::Union(_) | T::Literal(_) => spells_undef_with_generic_cell(&t.clone().lane()),
        // Str and Substr carry their own family oddballs; Any and a
        // dictionary carry the answer in the tag; the rest are scalars, absent
        // values, or names gone before lowering (`TypeVar` is substituted,
        // `Rest` is an annotation marker, `Object` is a global stand-in).
        T::String
        | T::Number
        | T::Boolean
        | T::Void
        | T::Never
        | T::Any
        | T::Dict(..)
        | T::Null
        | T::Undefined
        | T::Object(_)
        | T::TypeVar(_)
        | T::Rest(_) => false,
    }
}
