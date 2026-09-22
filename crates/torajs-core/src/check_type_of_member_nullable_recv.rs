//! What a possibly-undefined receiver decays to, for member lookup.
//!
//! §13.3.4 puts no static condition on the base of a property access:
//! the Reference Record is built unconditionally, and the TypeError
//! comes later from §6.2.5.5 GetValue -> §7.1.20 ToObject, at run
//! time. So `Nullable(T)` is not a reason to refuse a member read —
//! it is a reason to make sure the lowering-side guard is armed, and
//! those guards key on this same checker type.
//!
//! Three answers, and the file exists so that they are decided in one
//! place rather than at each member arm.

use crate::check::Type;
use crate::check_js_semantics::nullable_ptr_shaped;

/// What the member ladder should look the receiver up as.
pub(crate) enum Decay {
    /// Look the member up on this type.
    To(Type),
}

/// Decide, for one receiver type.
///
/// A `Nullable<T>` decays to `T`, and which T may is asked
/// by name: [`nullable_ptr_shaped`] is the question "is `Nullable<T>`
/// the same slot as `T`", and its own doc says to move all of its
/// sites together rather than spell the list again.
///
/// String is held back, and only here. Its read side is guarded
/// (`is_nullable_str_source` -> `__torajs_str_null_check`) but its
/// call side is not, and a str slot's absent value is a real str block
/// spelling "undefined" — decaying it would turn `s.toUpperCase()`
/// from a refusal into `"UNDEFINED"`. A refusal is wrong; a wrong
/// answer is worse. It decays once that guard exists.
pub(crate) fn decay(obj_ty: Type) -> Decay {
    match obj_ty {
        Type::Nullable(inner)
            if !matches!(*inner, Type::String)
                && nullable_ptr_shaped(&Type::Nullable(inner.clone())) =>
        {
            Decay::To(*inner)
        }
        other => Decay::To(other),
    }
}
