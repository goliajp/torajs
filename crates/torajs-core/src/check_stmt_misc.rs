//! Small `Stmt` arms (`Switch` / `Yield(Into)` / `Break-Continue` /
//! `ClassDecl` / `ImportDecl`) pulled out of
//! [`crate::check::Checker::check_stmt`] as chunk-100 of the
//! check_stmt decomp.
//!
//! Bundled because each is small and independent:
//!
//! - **`Stmt::Switch`** — scrutinee type vs case value types; nested
//!   scope per case body + default block. Walks each case's body
//!   recursively via `check_stmt`.
//! - **`Stmt::Yield` / `Stmt::YieldInto`** (Phase J) — `desugar_generators`
//!   rewrites generator bodies before typecheck; reaching here = a
//!   bare yield slipped through. Push typecheck error rather than
//!   panicking at SSA lower time.
//! - **`Stmt::Break` / `Stmt::Continue`** — no type-side state; the
//!   lowerer enforces in-loop placement.
//! - **`Stmt::ClassDecl`** (M5.1) — `desugar_classes` runs before
//!   check; reaching here is an internal panic (desugar pass bug).
//! - **`Stmt::ImportDecl`** (K.1 single-file mode) — parse-only, no
//!   semantic effect. K.2 will add cross-file symbol table check.

use std::collections::HashMap;

use crate::ast::{Ast, ExprId, Stmt, SwitchCase};
use crate::check::{Checker, DiagPush, Type};

pub(crate) fn check_switch(
    checker: &mut Checker,
    ast: &Ast,
    scrutinee: ExprId,
    cases: &[SwitchCase],
    default: &Option<Vec<Stmt>>,
) {
    let scrut_ty = match checker.type_of(ast, scrutinee) {
        Ok(t) => t,
        Err(e) => {
            checker.errors.push_err(e);
            return;
        }
    };
    for c in cases {
        match checker.type_of(ast, c.value) {
            Ok(t) if t == scrut_ty => {}
            // §14.12.4 selects a clause with IsStrictlyEqual, which is
            // total — it answers false across types rather than being
            // undefined for them. So the only thing worth refusing is
            // a pair that can never be equal, and `any` is never that
            // pair: it is exactly the type whose runtime value is
            // unknown. TypeScript agrees ("not comparable" is its
            // wording, and `any` is comparable to everything).
            //
            // Which the `with` desugar makes routine rather than
            // exotic: a guarded read is a conditional over the object,
            // so `switch (x)` in a `with` body has an `any` scrutinee
            // and every ordinary `case 1:` was refused.
            Ok(t) if t == Type::Any || scrut_ty == Type::Any => {}
            Ok(t) => checker.errors.push_err(format!(
                "switch case value type {t:?} differs from scrutinee {scrut_ty:?}"
            )),
            Err(e) => checker.errors.push_err(e),
        }
        checker.scopes.push(HashMap::new());
        for s in &c.body {
            checker.check_stmt(ast, s);
        }
        checker.scopes.pop();
        // ut3 — only one case body executes; no assignment narrow
        // crosses case boundaries.
        checker.flush_assign_narrows();
    }
    if let Some(db) = default {
        checker.scopes.push(HashMap::new());
        for s in db {
            checker.check_stmt(ast, s);
        }
        checker.scopes.pop();
        checker.flush_assign_narrows();
    }
}

pub(crate) fn check_yield(checker: &mut Checker) {
    checker
        .errors
        .push_err("yield is only valid inside a `function*` generator body".into());
}
