//! `Number.{isFinite,isNaN,isInteger,isSafeInteger}(value,
//! ...trailing)` predicate arm extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 241 — thirty-fourth sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! V3-18 wedge — per JS spec §21.1.2.2 / §21.1.2.4 /
//! §21.1.2.3 / §21.1.2.5: these methods do NOT coerce
//! their argument. They return `true` iff the arg is a
//! Number value AND satisfies the finite / NaN /
//! integer / safe-integer predicate; for non-Number
//! args (string / boolean / null / object / array)
//! they return `false` statically. The existing
//! signature `(Number) -> Boolean` rejects non-Number
//! args with a type error, but that's wrong for spec
//! and breaks the canonical TS feature-detection
//! idiom `if (Number.isFinite(maybeStringy)) ...`.
//!
//! - **S202** — extend the same loose check to the 0-arg
//!   form per §21.1.2.{3,5,7}: non-Number args (including
//!   the implicit undefined) statically return false;
//!   ssa_lower's short-circuit emits `ConstBool(false)`
//!   without dispatching the helper.
//! - **S253** — trailing-arg ignore per ES §21.1.2.{2,3,4,5}:
//!   spec reads only `args[0]`; tora silent-drops trailing
//!   per generic trailing-arg-ignore policy. SSA-emit
//!   short-circuits non-Number args (`ConstBool(false)`) and
//!   dispatches the helper for Number args, both reading
//!   only `args[0]`.
//!
//! Always returns `Type::Boolean` on match; `Some(Err(_))`
//! only if recursive `type_of` on args fails; `None` for
//! non-`Number.is{Finite,NaN,Integer,SafeInteger}` shapes.

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

pub(crate) fn try_match(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    args: &Vec<ExprId>,
) -> Option<Result<Type, String>> {
    let Expr::Member {
        obj: ns_id,
        name: m_name,
    } = ast.get_expr(*callee)
    else {
        return None;
    };
    let Expr::Ident(ns) = ast.get_expr(*ns_id) else {
        return None;
    };
    if ns != "Number"
        || !matches!(
            m_name.as_str(),
            "isFinite" | "isNaN" | "isInteger" | "isSafeInteger"
        )
    {
        return None;
    }
    // Force type_of on the arg so any internal typecheck error
    // still surfaces, but we don't require it to be Number —
    // non-Number args route through the lower's static-false
    // path.
    if let Some(arg0) = args.first() {
        if let Err(e) = checker.type_of(ast, *arg0) {
            return Some(Err(e));
        }
    }
    for &arg in args.iter().skip(1) {
        if let Err(e) = checker.type_of(ast, arg) {
            return Some(Err(e));
        }
    }
    Some(Ok(Type::Boolean))
}
