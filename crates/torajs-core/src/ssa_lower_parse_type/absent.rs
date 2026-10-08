//! The answers to "what spells the absent value in this slot" that
//! positions outside [`super::parse_type`] ask from the annotation
//! alone: the parser's implicit optional-parameter default and the
//! generator desugar's completion value.

/// Does `__nullable(<inner>)` materialize as `Type::Any` rather than
/// as the pointer-shaped in-band collapse?
///
/// A scalar `T | null` has nowhere to PUT the null — the in-band 0
/// sentinel is a legitimate `0` / `false` — so it pays the 8B NaN-box
/// tax instead (RFC 20260710-optional-undefined-repr C4). A
/// pointer-shaped T genuinely has the bit pattern to spare and stays
/// raw. This answers from the spelling alone; an alias RESOLVING to a
/// scalar is boxed by [`super::parse_type`] once it has resolved it.
///
/// Every position that has to spell "the absent value" for such a slot
/// asks HERE rather than restating the list — the parser's implicit
/// optional-parameter default is the second caller, and two spellings
/// of one rule is how the answers drift apart.
pub(crate) fn nullable_inner_boxes(inner: &str) -> bool {
    matches!(inner, "number" | "boolean")
}

/// Does the slot spelled `inner` materialize as `Type::Any` on its own
/// — the any family, and a union (whose value is NaN-boxed)? Such a
/// slot holds `undefined` as the plain ANY_UNDEF box, so the positions
/// that spell "the absent value" for it write the plain `undefined`,
/// never a per-type sentinel.
pub(crate) fn slot_is_any(inner: &str) -> bool {
    matches!(inner, "any" | "void" | "undefined") || inner.starts_with("__union(")
}
