//! Which receiver SHAPES an index read has to route through the
//! runtime dispatcher instead of a repr-specific load.
//!
//! Split out of [`crate::ssa_lower_index`] at rotation 621 under the
//! 500-line file cap. Verbatim move — `ssa_lower_index` re-exports
//! the name, so the eight call sites elsewhere are untouched.

use crate::ssa::Type;

/// Whether a receiver of this shape keeps an ordinary own-property bag
/// at runtime, and therefore boxes into the any lane so its keyed read
/// and write land in that bag.
///
/// The SSA-side mirror of torajs-anyvalue's
/// `member_get_layout::expando_props_off`, which is the authority:
/// that table decides where a shape's bag lives, this one decides
/// which shapes get to reach it. Both keyed channels here — the read
/// in this file and the write in `ssa_lower_index_assign` — ask this
/// one function, because they used to spell the same four-variant list
/// twice and a shape added to one would silently not reach the other.
///
/// `Date` / `Promise` / `MapIter` / `ArrIter` joined in rotation 611:
/// the runtime table has always had a slot for them, so the refusal
/// was this list's alone. `(new Date(0) as any)[0] = 1` died at
/// lowering while `(new Map() as any)[0] = 1` had just started
/// working one layer down.
///
/// WeakMap / WeakSet / WeakRef joined once they actually had a bag to
/// reach: when this predicate was first written their cells carried no
/// props slot, so admitting them would have moved the refusal one
/// layer down into member_set's reject rather than removing it (the
/// rotation 610-04 lesson). The following commit gave all three a
/// slot, and only then were they admitted — the order matters, and is
/// the whole difference between closing a seam and relocating it.
///
/// `Arr` is absent for a different reason: an array's expando lives
/// behind the `arrprops_*` kernels and its index keys are elements,
/// not bag entries — it takes the element lane above.
pub(crate) fn shape_has_expando_bag(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Obj(_)
            | Type::Map
            | Type::Set
            | Type::RegExp
            | Type::Date
            | Type::Promise
            | Type::MapIter
            | Type::ArrIter
            | Type::WeakMap
            | Type::WeakSet
            | Type::WeakRef
    )
}

/// §10.1.8.1 — indexing a PRIMITIVE is an ordinary [[Get]] through
/// its wrapper, so `(1)[0]` and `true[0]` answer undefined rather
/// than refusing to compile. These receivers carry no expando bag,
/// so they are not on the list above; they still have to reach the
/// runtime dispatcher, which answers by tag — `(1 as any)[0]` has
/// done exactly that all along, while the typed spelling died
/// "index access on non-array type I64".
///
/// Str and Substr are absent on purpose: real string indexing is
/// claimed by an earlier arm and returns a one-character Str.
pub(crate) fn primitive_index_receiver(ty: &Type) -> bool {
    matches!(
        ty,
        Type::I64 | Type::I32 | Type::F64 | Type::Bool | Type::BigInt
    )
}
