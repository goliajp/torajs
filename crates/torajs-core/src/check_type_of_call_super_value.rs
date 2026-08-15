//! RFC 20260815 knife 2b — `__torajs_super_value(P, this, [args…])`
//! synthetic call typecheck.
//!
//! The capturing lane rewrites a value-shaped parent's `super(…)`
//! into this synthetic so the checker sees a uniform `Expr::Call` and
//! ssa_lower can intercept by name to emit the runtime dispatch (a
//! class-cell parent routes to its registered `__ctorany_<P>` twin, a
//! closure takes [[Call]] with `this` bound, anything else raises
//! §15.7.14's TypeError — all decided by the kernel, not here).
//!
//! Returns `Some(Ok(Type::Undefined))` on the 3-arg shape (operand
//! `type_of` runs for side effects; every slot is any-world by
//! construction), `Some(Err(_))` on an operand failure, `None`
//! otherwise (cascade falls through).

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

pub(crate) fn try_match(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    args: &Vec<ExprId>,
) -> Option<Result<Type, String>> {
    let Expr::Ident(n) = ast.get_expr(*callee) else {
        return None;
    };
    // `__torajs_heritage_check(P)` (rotation 410) — the §15.7.14
    // step 5 class-definition-time gate; same synthetic family, one
    // any-world operand.
    let matches = (n == "__torajs_super_value" && args.len() == 3)
        || (n == "__torajs_heritage_check" && args.len() == 1);
    if !matches {
        return None;
    }
    for a in args {
        if let Err(e) = checker.type_of(ast, *a) {
            return Some(Err(e));
        }
    }
    Some(Ok(Type::Undefined))
}
