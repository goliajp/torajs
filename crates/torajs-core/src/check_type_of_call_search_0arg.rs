//! Array / String search-method 0-arg arity narrow wedge
//! extracted from [`crate::check_type_of_call::check`]'s
//! post-cascade generic-call mechanics (chunk 303 —
//! ninety-fifth sub-batch of check_type_of_call.rs
//! per-shape decomposition).
//!
//! Per ES §22.1.3.{8,13,...} and §23.1.3.{14,17,18}:
//! `searchElement` / `searchString` defaults to undefined
//! when omitted (algorithm steps read the missing argument
//! as undefined). The declared `Type::Function` sig is the
//! 1-arg full form; truncate `params` to 0 on the no-arg
//! call so the strict-equality arity check that follows
//! accepts it.
//!
//! SSA emit semantics:
//!   String: fill needle = `ToString(undefined) =
//!     "undefined"` and search normally (bun observable).
//!   Array<T> with T ≠ Any: undefined cannot strict-equal
//!     any typed element, so the result is fixed
//!     (-1 / false). Array<Any> is omitted from the
//!     truncate here because the search would need an
//!     Any-tagged undefined sentinel; L3b.
//!
//! Method allowlist:
//!   String: indexOf, includes, lastIndexOf, startsWith,
//!           endsWith
//!   Array<T≠Any>: indexOf, includes, lastIndexOf
//!
//! Mutator-shape sibling (mirrors chunk 302 array_at_narrow):
//! returns `Result<(), String>`, read-only borrow of
//! `effective_args` because the wedge only truncates
//! `params`.
//!
//! Returns:
//! - `Ok(())` — wedge applied (params truncated) or did
//!   not fire (effective_args non-empty, callee not a
//!   Member, recv/method combo outside allowlist, or
//!   Array<Any>)
//! - `Err(_)` — receiver `type_of` failed

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

pub(crate) fn apply(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    effective_args: &[ExprId],
    params: &mut Vec<Type>,
) -> Result<(), String> {
    if !effective_args.is_empty() {
        return Ok(());
    }
    let Expr::Member { obj, name } = ast.get_expr(*callee) else {
        return Ok(());
    };
    let recv_ty = checker.type_of(ast, *obj)?;
    let trunc = match (&recv_ty, name.as_str()) {
        (Type::String, "indexOf" | "includes" | "lastIndexOf" | "startsWith" | "endsWith") => true,
        (Type::Array(elem), "indexOf" | "includes" | "lastIndexOf") => **elem != Type::Any,
        _ => false,
    };
    if trunc {
        params.truncate(0);
    }
    Ok(())
}
