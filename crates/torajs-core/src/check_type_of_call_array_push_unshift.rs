//! `xs.{push,unshift}(...vals)` variadic arm extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 235 — twenty-eighth sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! V3-18 wedge — Array.push / Array.unshift accept a variable
//! number of args per JS spec §22.1.3.20 / §22.1.3.34. Each
//! arg is appended (or prepended) in order. Pre-fix tora's
//! strict 1-arg signature rejected the multi-arg form. Subset
//! typecheck enforces every arg matches the element type and
//! returns Void (push's new-length return is not surfaced).
//!
//! Returns `Some(Ok(Void))` on Array-receiver match with all
//! args type-compatible; `Some(Err(_))` on type mismatch;
//! `None` otherwise (non-Array receiver, non-{push,unshift},
//! or arity = 1 which falls to the generic arg-unify path).

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

pub(crate) fn try_match(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    args: &Vec<ExprId>,
) -> Option<Result<Type, String>> {
    let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
    else {
        return None;
    };
    if !matches!(m_name.as_str(), "push" | "unshift") || args.len() == 1 {
        return None;
    }
    let src_ty = match checker.type_of(ast, *src_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    let Type::Array(elem) = src_ty else {
        return None;
    };
    let inner = (*elem).clone();
    for (i, aid) in args.iter().enumerate() {
        let aty = match checker.type_of(ast, *aid) {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        if aty != inner && aty != Type::Any {
            return Some(Err(format!(
                "Array.{m_name} arg {i}: expected element type {:?}, got {aty:?}",
                inner
            )));
        }
    }
    Some(Ok(Type::Void))
}
