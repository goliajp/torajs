//! `Stmt::Try { body, catch_param, catch_type, catch_body,
//! finally_body, .. }` typecheck pulled out of
//! [`crate::check::Checker::check_stmt`]'s `Stmt::Try` arm as
//! chunk-104 of the check_stmt decomp. 7th check_stmt sibling.
//!
//! 3-phase walker:
//!
//! 1. **Body** — fresh scope; walk each stmt via `check_stmt`.
//! 2. **Catch** — fresh scope with `e` bound. Param type from
//!    `(e: T)` annotation; P7.2b-2 — unannotated `catch (e)` is
//!    implicitly `any` per TS spec (an explicit non-any/unknown
//!    annotation is TS1196). Unknown type ann → push error + fall
//!    back to Any. Default `None => Any` mirrors ssa_lower so
//!    check-tier sees `e` as Any too (member access / arithmetic
//!    / return all go through Any paths).
//! 3. **Finally** (optional) — fresh scope; walk each stmt.

use std::collections::HashMap;

use crate::ast::{Ast, Stmt};
use crate::check::{Checker, DiagPush, LocalInfo, Type};
use crate::check_type_ann::resolve_type_ann_full;

pub(crate) fn check(
    checker: &mut Checker,
    ast: &Ast,
    body: &[Stmt],
    catch_param: &Option<String>,
    catch_type: &Option<String>,
    catch_body: &[Stmt],
    finally_body: &Option<Vec<Stmt>>,
) {
    // body in a fresh scope
    checker.scopes.push(HashMap::new());
    for s in body {
        checker.check_stmt(ast, s);
    }
    checker.scopes.pop();
    // ut3 — the catch entry point is any throw site inside the
    // body: no body assignment narrow survives into catch.
    checker.flush_assign_narrows();
    // catch in a fresh scope with `e` injected.
    let e_ty = match catch_type {
        Some(ann) => {
            match resolve_type_ann_full(ann, &checker.aliases, &[], &checker.generic_alias_decls) {
                Some(t) => t,
                None => {
                    checker
                        .errors
                        .push_err(format!("unknown type `{ann}` in catch param"));
                    Type::Any
                }
            }
        }
        None => Type::Any,
    };
    checker.scopes.push(HashMap::new());
    if let Some(p) = catch_param {
        let _ = checker.declare(
            p.clone(),
            LocalInfo {
                ty: e_ty,
                mutable: true,
                moved: false,
                borrowed: false,
                declared_class: None,
            },
        );
    }
    for s in catch_body {
        checker.check_stmt(ast, s);
    }
    checker.scopes.pop();
    checker.flush_assign_narrows();
    if let Some(fb) = finally_body {
        checker.scopes.push(HashMap::new());
        for s in fb {
            checker.check_stmt(ast, s);
        }
        checker.scopes.pop();
        checker.flush_assign_narrows();
    }
}
