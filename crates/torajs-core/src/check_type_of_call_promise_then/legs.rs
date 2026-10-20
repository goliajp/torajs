//! The value type a two-handler `.then` derives, from the two handlers'
//! returns.

use crate::check::Type;

/// The value type of the promise `.then(onOk, onErr)` derives: §27.2.5.4
/// PerformPromiseThen settles it from whichever handler runs, so it is
/// fulfilled with onOk's return or onErr's (each adopted, §27.2.1.3.2) —
/// their union. A throw-only handler (`never`) settles nothing on its
/// leg, so the other leg alone answers; an undefined that a void handler
/// answers joins as `undefined`. When both legs agree the answer is that
/// one type, as it always was.
pub(super) fn two_leg_result([ok, err]: [Type; 2]) -> Type {
    let unwrap = |t: Type| match t {
        Type::Promise(inner) => *inner,
        Type::Void => Type::Undefined,
        other => other,
    };
    if err == ok || err == Type::Never {
        return ok;
    }
    if ok == Type::Never {
        return err;
    }
    let ok_v = unwrap(ok.clone());
    let err_v = unwrap(err);
    if ok_v == err_v {
        return ok;
    }
    Type::union_of([ok_v, err_v])
}
