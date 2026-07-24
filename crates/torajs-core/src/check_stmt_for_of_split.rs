//! `Stmt::ForOfSplitIter { var_name, parent, sep, body }` typecheck
//! pulled out of [`crate::check::Checker::check_stmt`]'s
//! `Stmt::ForOfSplitIter` arm as chunk-105 of the check_stmt decomp.
//! 8th check_stmt sibling.
//!
//! P-iter — `parent` and `sep` must both be strings; `var_name`
//! binds a `Substr`-shaped String borrow per iteration. The walker:
//!
//! 1. Typecheck parent — must be String.
//! 2. Typecheck sep — must be String.
//! 3. Push fresh scope; declare `var_name: String` (const,
//!    non-moved, non-borrowed).
//! 4. check_stmt body.
//! 5. Pop scope.

use std::collections::HashMap;

use crate::ast::{Ast, ExprId, Stmt};
use crate::check::{Checker, DiagPush, LocalInfo, Type};

pub(crate) fn check(
    checker: &mut Checker,
    ast: &Ast,
    var_name: &str,
    parent: ExprId,
    sep: ExprId,
    body: &Stmt,
) {
    match checker.type_of(ast, parent) {
        Ok(Type::String) => {}
        Ok(other) => checker
            .errors
            .push_err(format!("for-of split parent must be string, got {other:?}")),
        Err(e) => checker.errors.push_err(e),
    }
    match checker.type_of(ast, sep) {
        Ok(Type::String) => {}
        Ok(other) => checker.errors.push_err(format!(
            "for-of split separator must be string, got {other:?}"
        )),
        Err(e) => checker.errors.push_err(e),
    }
    checker.scopes.push(HashMap::new());
    let _ = checker.declare(
        var_name.to_string(),
        LocalInfo {
            ty: Type::String,
            mutable: false,
            moved: false,
            borrowed: false,
            declared_class: None,
            builtin_mv: false,
        },
    );
    checker.check_stmt(ast, body);
    checker.scopes.pop();
}
