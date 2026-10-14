//! Whether a value of a checked type may hold a definer, and a key of
//! one may spell a definer's name: a computed-key read the census left
//! open reaches every name only then. Moved out of [`super`].

use super::DEFINERS;

pub(super) fn may_hold_definer(t: &crate::check::Type) -> bool {
    use crate::check::Type;
    match t {
        Type::Any | Type::Function(..) | Type::Object(_) | Type::TypeVar(_) | Type::Dict(..) => {
            true
        }
        Type::Nullable(t) => may_hold_definer(t),
        Type::Union(ts) => ts.iter().any(may_hold_definer),
        _ => false,
    }
}

/// Whether a key of this type can be a definer's name: a string built
/// at run time, or the name itself as a literal type (`k as
/// "defineProperty"`, which spells no string expression).
pub(super) fn may_spell_definer(t: &crate::check::Type) -> bool {
    use crate::check::{Lit, Type};
    match t {
        Type::Any | Type::String | Type::TypeVar(_) => true,
        Type::Literal(Lit::Str(s)) => DEFINERS.iter().any(|d| s == *d),
        Type::Nullable(t) => may_spell_definer(t),
        Type::Union(ts) => ts.iter().any(may_spell_definer),
        _ => false,
    }
}
