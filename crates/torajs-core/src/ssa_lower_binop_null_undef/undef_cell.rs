//! The immortal cell each slot type spells `undefined` with. Moved out
//! of [`super`].

use super::UNDEF_CELL_SYM;
use crate::ssa::Type;
use crate::ssa_lower_intrinsics_str_b::STR_UNDEF_CELL_SYM;
use crate::ssa_lower_intrinsics_substr::SUBSTR_UNDEF_CELL_SYM;

/// The immortal cell a slot of type `slot_ty` spells `undefined` with;
/// `None` for a slot that has none.
pub(crate) fn undef_cell_sym(slot_ty: Type) -> Option<&'static str> {
    Some(match slot_ty {
        Type::Str => STR_UNDEF_CELL_SYM,
        Type::Substr => SUBSTR_UNDEF_CELL_SYM,
        // RFC 20260710 C2a — a fn-typed slot is Copy (no rc/drop
        // traffic), so the Str-shaped oddball serves as its
        // undefined repr directly: eq compares the address,
        // print/ToString consult the identity probe.
        Type::FnSig(_) => STR_UNDEF_CELL_SYM,
        // RFC 20260710 C2b — refcounted pointer slots take the
        // generic Tag::Undefined cell: the drop stations guard
        // on it (nullish, not just NULL), strict-eq compares
        // the address, print/JSON probe identity, and the any
        // boundary re-encodes it as ANY_UNDEF.
        t if t.spells_undef_with_generic_cell() => UNDEF_CELL_SYM,
        _ => return None,
    })
}
