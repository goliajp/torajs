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
/// did not type. One case is left out on purpose there: a join that may
/// settle undefined into a pointer slot gives no seed, so the next
/// handler takes its parameter on the any lane until the settle path
/// can hand a pointer slot undefined.
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
