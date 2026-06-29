//! `xs.{sort,toSorted}(cmp?, ...trailing)` 0/1/2+-arg
//! Array-receiver arm extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 238 — thirty-first sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! V3-18 wedge — Array.sort / toSorted accept an optional
//! comparator per JS spec §22.1.3.27. Pre-fix tora's strict
//! 1-arg signature rejected the no-arg form `arr.sort()`.
//!
//! Three sub-shapes folded here:
//!
//! - **0-arg** — fall to default lex/`<`/`>` compare; result
//!   inherits the receiver's element type.
//! - **1-arg `Undefined`** (S303) — per ES §23.1.3.{29,31}
//!   step 1 `undef` is equivalent to no comparator; SSA mirror
//!   at ssa_lower_str's `sort/toSorted` dispatch reuses the
//!   default path. (1-arg non-Undefined falls through to the
//!   regular comparator-type-check path.)
//! - **S276** 2+-arg `(cmp, ...trailing)` — per ES §23.1.3.
//!   {30,33} trailing-arg ignore. Accept any `cmp` shape
//!   (`Function | Any | Undefined`), typecheck-and-drop
//!   trailing exprs.
//!
//! Returns `Some(Ok(Array<elem>))` on matched Array-receiver
//! shapes; `Some(Err(_))` on type_of error in a checked arg;
//! `None` otherwise (non-Array receiver, non-{sort,toSorted},
//! or 1-arg non-Undefined which falls to the regular path).

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
    if !matches!(m_name.as_str(), "sort" | "toSorted") {
        return None;
    }
    if args.is_empty() {
        let src_ty = match checker.type_of(ast, *src_id) {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        if let Type::Array(elem) = src_ty {
            return Some(Ok(Type::Array(elem)));
        }
        return None;
    }
    if args.len() == 1 {
        let arg_ty = match checker.type_of(ast, args[0]) {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        if arg_ty != Type::Undefined {
            return None;
        }
        let src_ty = match checker.type_of(ast, *src_id) {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        if let Type::Array(elem) = src_ty {
            return Some(Ok(Type::Array(elem)));
        }
        return None;
    }
    // args.len() >= 2 — S276 trailing-arg ignore + S303 Undefined cmp.
    let src_ty = match checker.type_of(ast, *src_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    let Type::Array(elem) = src_ty else {
        return None;
    };
    let aty0 = match checker.type_of(ast, args[0]) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if !matches!(aty0, Type::Function(..) | Type::Any | Type::Undefined) {
        return None;
    }
    for &a in &args[1..] {
        if let Err(e) = checker.type_of(ast, a) {
            return Some(Err(e));
        }
    }
    Some(Ok(Type::Array(elem)))
}
