//! §27.2.5.4 step 3/4 — a promise reaction handler that is not
//! callable is an EMPTY slot, not an error.
//!
//! `p.then(undefined)` and `p.then({})` are promises that resolve:
//! PerformPromiseThen leaves the handler empty and the source value
//! is forwarded. §27.2.5.1 `catch` and §27.2.5.3 `finally` say the
//! same of theirs. This is the opposite of what the Array callback
//! methods say (§23.1.3.x throw), and the two rules meet in the same
//! checker — so which one applies is decided by the method, never by
//! the parameter type.
//!
//! tr's typed promise arms fix the handler's signature, so anything
//! that is not a matching `Function` was refused: a callable Proxy,
//! a class object, `undefined`, `{}`. The any lane has answered all
//! of them correctly since rotation 682 — `method_call_promise`
//! resolves each handler through `callback_resolve` (§7.2.3) and
//! treats a None as the empty slot the spec describes, forwarding
//! the source value with its own repr. What was missing is the
//! checker's permission to send the call there.
//!
//! This is the `check_type_of_call_arr_hof_any_cb` move, applied to
//! the other family: record the callee as Any so
//! `ssa_lower_any_method_call` claims the whole call, and let the
//! runtime decide. A handler that IS a matching function is not
//! touched — the typed arms run right after this one and keep their
//! chain lowering, so nothing that compiles today changes route.

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

/// `Some(Ok(Any))` when a promise reaction method is handed at least
/// one handler that is not statically a function, and the receiver
/// is a promise. `None` otherwise — every typed shape falls through
/// to the arms below.
pub(crate) fn try_match(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    args: &[ExprId],
) -> Option<Result<Type, String>> {
    let Expr::Member { obj, name } = ast.get_expr(*callee) else {
        return None;
    };
    if !matches!(name.as_str(), "then" | "catch" | "finally") || args.is_empty() {
        return None;
    }
    let Ok(obj_ty) = checker.type_of(ast, *obj) else {
        return None;
    };
    if !matches!(obj_ty, Type::Promise(_)) {
        return None;
    }
    // Every argument is typed either way: a handler position is an
    // ordinary expression and its own errors are the program's.
    let mut tys: Vec<Type> = Vec::with_capacity(args.len());
    for a in args {
        match checker.type_of(ast, *a) {
            Ok(t) => tys.push(t),
            Err(e) => return Some(Err(e)),
        }
    }
    // A `Function` handler — whatever its signature — belongs to the
    // typed arms; only a shape they cannot express comes here. `Any`
    // is one of those: it is the Proxy / class / unknown spelling,
    // and the typed chain would take its bits for a function
    // pointer.
    if tys.iter().all(|t| matches!(t, Type::Function(..))) {
        return None;
    }
    checker.expr_types.insert(*callee, Type::Any);
    Some(Ok(Type::Any))
}
