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
/// String decays like the rest. Its absent value is spelled
/// differently (NULL, or an immortal str block reading "undefined"),
/// so it has its own guard kernel, `__torajs_str_null_check`, armed
/// on reads by `is_nullable_str_source` and on method calls by
/// `ssa_lower_call_undef_recv::arm`. Until the call side existed this
/// line held String back, because decaying it would have turned
/// `s.toUpperCase()` from a refusal into `"UNDEFINED"`.
pub(crate) fn decay(obj_ty: Type) -> Decay {
    if matches!(obj_ty, Type::Nullable(_)) && rides_any_lane(&obj_ty) {
        return Decay::AnyLane;
    }
    match obj_ty {
        Type::Nullable(inner) if nullable_ptr_shaped(&Type::Nullable(inner.clone())) => {
            Decay::To(*inner)
        }
        other => Decay::To(other),
    }
}
