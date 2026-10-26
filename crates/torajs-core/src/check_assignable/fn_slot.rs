//! The fn-face admit of a declared fn-typed slot.

use crate::check::Type;

/// 423-03 ④ — the fn-face admit for a DECLARED fn-typed slot (let /
/// assign positions): the S2 call-face relaxations (excess-Any-tail
/// arity, void-formal return) apply, because the sig-thunk pass
/// reabstracts the admitted mismatched value exactly like a call
/// argument. Deliberately a SEPARATE predicate from the
/// `is_assignable_to` fn arm above — that one also serves
/// array-literal unification, where the wider admit collapsed
/// previously-heterogeneous literals (the rotation-355 regression).
pub(crate) fn fn_slot_admits(
    to: &Type,
    from: &Type,
    class_structs: &std::collections::HashMap<String, Type>,
    aliases: &std::collections::HashMap<String, Type>,
    generic_aliases: &crate::check::GenericAliasMap,
) -> bool {
    matches!(to, Type::Function(..))
        && crate::check_type_of_call_callback_subtype::matches_resolved(
            to,
            from,
            class_structs,
            aliases,
            generic_aliases,
        )
}
