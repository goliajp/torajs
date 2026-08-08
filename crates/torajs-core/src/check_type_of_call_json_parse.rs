//! `JSON.parse(text, reviver?)` arity wedge (RFC
//! 20260808-json-parse-any blade 3).
//!
//! The namespace sig (`check_type_of_member_namespace`) spells the
//! 1-arg shape; §25.5.1 also takes an optional reviver (step 7 gates
//! on IsCallable at runtime — a non-callable second argument is
//! silently unfiltered, so the checker admits Any there) and the
//! 0-arg spelling (ToString(undefined) = `"undefined"`, a runtime
//! SyntaxError, not a compile reject). Everything past slot 2 is
//! evaluated for side effects and ignored, the S311 stringify shape.
//!
//! Returns `Some(Ok(Any))` on the `JSON.parse` Member-Ident callee;
//! `Some(Err(_))` on an argument that fails to type; `None`
//! otherwise.

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

pub(crate) fn try_match(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    args: &Vec<ExprId>,
) -> Option<Result<Type, String>> {
    if let Expr::Member { obj, name } = ast.get_expr(*callee)
        && let Expr::Ident(ns) = ast.get_expr(*obj)
        && ns == "JSON"
        && name == "parse"
    {
        for &aid in args {
            if let Err(e) = checker.type_of(ast, aid) {
                return Some(Err(e));
            }
        }
        return Some(Ok(Type::Any));
    }
    None
}
