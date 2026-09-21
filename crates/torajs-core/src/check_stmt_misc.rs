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

/// `true` when a `case undefined:` / `case null:` clause CAN be
/// selected under this scrutinee, whatever the checker's type for
/// it says.
///
/// The "never selected" warning next door rests on the type being
/// the whole truth about what the slot holds, and for a
/// pointer-shaped one it is not: the immortal cell and the null
/// pointer are both in-band values of that slot, and they arrive
/// by roads the type does not record — a `find` miss, `pop` on an
/// empty array, an unwritten field, an out-of-range index. The
/// checker types all of those as a bare `T`.
///
/// So the warning was telling the programmer the opposite of the
/// truth for exactly the clause that catches those values, while
/// the run-time answer (once the lowering compares against the
/// right address) is that the clause IS selected.
fn nullish_clause_is_reachable(scrut_ty: &Type, clause_ty: &Type) -> bool {
    if !matches!(clause_ty, Type::Undefined | Type::Null) {
        return false;
    }
    matches!(
        scrut_ty,
        Type::Struct(_)
            | Type::ClassRef(_)
            | Type::Array(_)
            | Type::Nullable(_)
            | Type::Date
            | Type::RegExp
            | Type::Map
            | Type::Set
            | Type::Promise(_)
            | Type::WeakMap
            | Type::WeakSet
            | Type::WeakRef
            | Type::MapIter
            | Type::ArrIter
    )
}


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
    // §14.12.4 — the CaseBlock is ONE declarative environment for
    // every clause plus the default, not one per clause. Per-clause
    // frames made `case 0: let a = 1; case 1: a` an unknown
    // identifier and let a clause-declared name escape the switch.
    checker.scopes.push(HashMap::new());
    for c in cases {
        match checker.type_of(ast, c.value) {
            // `any` on either side is the type whose runtime value is
            // unknown, so nothing can be said about it here.
            Ok(t)
                if t == scrut_ty
                    || t == Type::Any
                    || scrut_ty == Type::Any
                    // A `T | undefined` scrutinee has three faces a
                    // clause can equal: the T, `undefined`, and
                    // `null` — the lattice spells the last two as
                    // one slot but IsStrictlyEqual keeps them apart.
                    // Warning "never selected" on `case undefined:`
                    // under exactly the type that exists to carry
                    // undefined said the opposite of the truth.
                    || matches!(&scrut_ty, Type::Nullable(inner)
                        if t == Type::Undefined || t == Type::Null || t == **inner)
                    || nullish_clause_is_reachable(&scrut_ty, &t) => {}
            // §14.12.4 selects a clause with IsStrictlyEqual, and that
            // predicate is TOTAL: across types it answers `false`, it is
            // not undefined for them. So no pair of types is an error
            // here — a cross-type clause is simply one that never gets
            // selected, and §14.12.4 moves to the next.
            //
            // The refusal that used to stand here had the reasoning
            // right and stopped halfway: it opened a hole for `any` on
            // exactly this ground ("any is comparable to everything")
            // and then refused every other cross-type pair. Which is
            // band B in
            // `docs/language-spec/ch1-type-runtime-boundary.md` — a
            // runtime answer treated as a compile-time error.
            //
            // TypeScript does flag part of this — `case 0:` under a
            // string scrutinee is TS2678 "not comparable" (`case null:`
            // is not). That diagnostic has value and is kept; what is
            // dropped is the REFUSAL. This is exactly the row ch1 §4
            // draws between the two: tsc reports and produces no
            // executable, tr reports AND emits the behaviour the spec
            // asks for. The undeclared-identifier path has been shaped
            // this way for a while — a warning, and the program runs.
            //
            // The typed spelling is where tr is AHEAD here, not behind:
            // with the scrutinee's type in hand, "never selected" is
            // decidable at compile time, while an engine that erased the
            // type has to run the comparison to find out. What must not
            // be skipped is the clause EXPRESSION — CaseClauseIsSelected
            // evaluates it before comparing, so a side effect in a
            // never-matching case is observable (the fixture pins this).
            Ok(t) => checker.errors.push_warn(format!(
                "switch case value type {t:?} can never equal scrutinee \
                 {scrut_ty:?} — this clause is never selected"
            )),
            Err(e) => checker.errors.push_err(e),
        }
        for s in &c.body {
            checker.check_stmt(ast, s);
        }
        // ut3 — only one case body executes; no assignment narrow
        // crosses case boundaries.
        checker.flush_assign_narrows();
    }
    if let Some(db) = default {
        for s in db {
            checker.check_stmt(ast, s);
        }
        checker.flush_assign_narrows();
    }
    checker.scopes.pop();
}

pub(crate) fn check_yield(checker: &mut Checker) {
    checker
        .errors
        .push_err("yield is only valid inside a `function*` generator body".into());
}
