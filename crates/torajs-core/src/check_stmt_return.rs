//! `Stmt::Return(maybe_expr)` typecheck pulled out of
//! [`crate::check::Checker::check_stmt`]'s `Stmt::Return` arm as
//! chunk-99 of the check_stmt decomp.
//!
//! Rules preserved exactly:
//!
//! 1. **Outside fn** — `expected_return` is `None`: push error
//!    "return outside of a function".
//! 2. **Actual type** — bare `return` → `Type::Void`; `return expr`
//!    → `type_of(expr)`; type-of error propagated to errors list.
//! 3. **V3-18 Nullable<T> return wedge** — expected `Nullable<T>`
//!    accepts both `T`-typed and `Null` values (mirrors call-arg
//!    widening for params).
//! 4. **P0.9 assignability check** — return-type uses the
//!    assignability lattice so Any-typed expected (or structs
//!    containing Any fields) accept concrete returned values.
//!    Previous strict-eq blocked default-Any generators returning
//!    concrete iterator-result structs.
//! 5. **Move-out** — returning a non-Copy ident moves it out to the
//!    caller; borrowed aliases may escape (retain-at-return gives
//!    the caller its own +1).

use crate::ast::{Ast, ExprId};
use crate::check::{Checker, DiagPush, Type};
use crate::check_assignable::is_assignable_to_resolved;

pub(crate) fn check(checker: &mut Checker, ast: &Ast, maybe_expr: Option<ExprId>) {
    let Some(expected) = checker.expected_return.clone() else {
        checker
            .errors
            .push_err("`return` outside of a function".into());
        return;
    };
    let actual = match maybe_expr {
        None => Type::Void,
        Some(eid) => match checker.type_of(ast, eid) {
            Ok(t) => t,
            Err(e) => {
                checker.errors.push_err(e);
                return;
            }
        },
    };
    let nullable_ok = if let Type::Nullable(inner) = &expected {
        actual == Type::Null || actual == **inner
    } else {
        false
    };
    if !nullable_ok
        && !is_assignable_to_resolved(
            &expected,
            &actual,
            &checker.aliases,
            &checker.generic_alias_decls,
        )
    {
        checker.errors.push_err(format!(
            "return type mismatch: function expects {expected:?}, got {actual:?}"
        ));
    }
    if let Some(eid) = maybe_expr
        && !expected.is_copy()
    {
        checker.consume_escape(ast, eid);
    }
}
