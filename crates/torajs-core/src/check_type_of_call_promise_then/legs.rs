//! The value type a two-handler `.then` derives, from the two handlers'
//! returns.

use crate::check::Type;

/// The value type of the promise `.then(onOk, onErr)` derives: §27.2.5.4
/// PerformPromiseThen settles it from whichever handler runs, so it is
/// fulfilled with onOk's return or onErr's (each adopted, §27.2.1.3.2) —
/// their union. A throw-only handler (`never`) settles nothing on its
/// leg, so the other leg alone answers; a void handler settles
/// undefined. When both legs agree the answer is that one value.
///
/// The seed for the next handler's parameter joins the same way
/// (`join_legs` in `ast::infer_closure_params_promise`); the two must
/// answer alike or the handler reads a value through a lane the checker
/// did not type.
pub(super) fn two_leg_result([ok, err]: [Type; 2]) -> Type {
    let value = |t: Type| match t {
        Type::Never => None,
        Type::Void => Some(Type::Undefined),
        Type::Promise(inner) => Some(*inner),
        other => Some(other),
    };
    match (value(ok), value(err)) {
        (Some(a), Some(b)) if a == b => a,
        (Some(a), Some(b)) => Type::union_of([a, b]),
        (Some(v), None) | (None, Some(v)) => v,
        (None, None) => Type::Undefined,
    }
}

/// Does a handler parameter of type `p` run on a typed lane — one a
/// settled value can fail to fit, so that the handler's any twin runs
/// in its place? `any`, a union and a nullable take every value on the
/// any lane, and such a handler's declared return is what it settles.
pub(super) fn typed_lane(p: &Type) -> bool {
    !matches!(p, Type::Any | Type::Union(_) | Type::Nullable(_))
}
