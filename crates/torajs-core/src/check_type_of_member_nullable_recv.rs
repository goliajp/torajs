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
use crate::check_js_semantics::{nullable_ptr_shaped, rides_any_lane};

/// What the member ladder should look the receiver up as.
pub(crate) enum Decay {
    /// Read it the way an `any` receiver is read.
    AnyLane,
    /// Look the member up on this type.
    To(Type),
}

/// Decide, for one receiver type.
///
/// A scalar's nullable already lives in the NaN-boxed any lane
/// ([`rides_any_lane`]) — the box tax its slot pays is exactly what
/// lets it hold undefined — so the member read has an answer and it is
/// the any lane's. Asking it here is the move the binary and unary
/// operand lanes already make; without it `n.toFixed(2)` on a `number
/// | undefined` was refused while `n + 1` on the same slot answered,
/// which is not a distinction the spec draws.
///
/// Every other `Nullable<T>` decays to `T`, and which T may is asked
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
    if matches!(obj_ty, Type::Nullable(_)) && rides_any_lane(&obj_ty) {
        return Decay::AnyLane;
    }
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
