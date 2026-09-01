//! `s.concat(...others)` arms (arity ≠ 1 + the 1-arg Undefined
//! widen) extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 226 — nineteenth sub-batch of check_type_of_call.rs
//! per-shape decomposition).
//!
//! Both arms gate on `Type::String` receiver:
//!
//! - **arity ≠ 1** — variadic concatenation; each arg must be
//!   `String` or `Type::Undefined`. Empty arg list returns the
//!   receiver unchanged at lower-time. S212 explicit
//!   `undefined` arg per ES §22.1.3.4 step 3.a:
//!   `ToString(undefined) = "undefined"`; ssa_lower inline-
//!   substitutes the interned "undefined" literal.
//! - **arity == 1 + Undefined arg** — narrow widen so
//!   ssa_lower's inline-undef substitution can run. The
//!   declared `Function` arm is strict-`String` and would
//!   reject the typed-`Undefined` operand. Other 1-arg shapes
//!   (String / Any / etc.) fall through to the general table
//!   here (`None`).
//!
//! Returns `Some(Ok(String))` on match; `Some(Err(_))` on
//! non-String-non-Undefined varargs arg; `None` when callee
//! isn't a `s.concat(...)` on a `Type::String` receiver, or
//! it's the arity-1 non-Undefined case that the general table
//! handles.

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

pub(crate) fn try_match(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    args: &Vec<ExprId>,
) -> Option<Result<Type, String>> {
    let Expr::Member {
        obj: recv_id,
        name: m,
    } = ast.get_expr(*callee)
    else {
        return None;
    };
    if m != "concat" {
        return None;
    }
    if !matches!(checker.type_of(ast, *recv_id), Ok(Type::String)) {
        return None;
    }
    if args.len() != 1 {
        for &aid in args {
            let aty = match checker.type_of(ast, aid) {
                Ok(t) => t,
                Err(e) => return Some(Err(e)),
            };
            // §22.1.3.5 step 3.b ToString's every argument — an Any
            // actual (a String wrapper object, a boxed primitive) is
            // admitted here exactly as the arity-1 general-table path
            // admits it; the lower dispatch routes it through
            // `any_to_str_box`. Rejecting it only on the variadic
            // spelling made `s.concat(Object(5), "z")` a type error
            // while `s.concat(Object(5))` compiled (552-02).
            if matches!(aty, Type::Undefined | Type::Any) {
                continue;
            }
            if aty != Type::String {
                return Some(Err(format!(
                    "String.concat args must be string, got {aty:?}"
                )));
            }
        }
        return Some(Ok(Type::String));
    }
    // arity == 1 — only fire on Undefined; other shapes
    // fall through to the general Type::Function table.
    let aty = match checker.type_of(ast, args[0]) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if matches!(aty, Type::Undefined) {
        return Some(Ok(Type::String));
    }
    None
}
