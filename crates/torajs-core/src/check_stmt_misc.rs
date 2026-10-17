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
use crate::check::{Checker, DiagPush, FlowState, Type, stmt_diverges};

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
            | Type::Map(..)
            | Type::Set
            | Type::Promise(_)
            | Type::WeakMap
            | Type::WeakSet
            | Type::WeakRef
            | Type::MapIter(..)
            | Type::ArrIter
    )
}

pub(crate) fn check_switch(checker: &mut Checker, ast: &Ast, stmt: &Stmt) {
    let Stmt::Switch {
        scrutinee,
        cases,
        default,
        default_at,
    } = stmt
    else {
        return;
    };
    // `switch (u.kind)` with literal cases narrows `u` per clause; any
    // other scrutinee naming a union binding leaves it loose
    let narrows = checker.switch_narrows(ast, *scrutinee, cases, default.as_deref(), *default_at);
    if narrows.is_none() {
        checker.note_unmodeled_guard(ast, *scrutinee);
    }
    let clause_narrow = |i: Option<usize>| {
        narrows.as_ref().map(|(n, per_case, on_default)| {
            (n.clone(), i.map_or(on_default, |i| &per_case[i]).clone())
        })
    };
    // `switch (true) { case typeof y === "string": … }` guards in a case
    for c in cases {
        checker.note_unmodeled_guard(ast, c.value);
    }
    let scrut_ty = match checker.type_of(ast, *scrutinee) {
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
    // One list for the closure-capture hoist too: a closure in an
    // earlier clause may name a binding a later clause declares, and
    // falling through runs that declaration before the closure is
    // called (the dead zone itself is settled at run time by
    // `switch_tdz_flag`).
    let block: Vec<Stmt> = {
        let (before, after) = cases.split_at(*default_at);
        before
            .iter()
            .map(|c| c.body.as_slice())
            .chain(default.as_deref())
            .chain(after.iter().map(|c| c.body.as_slice()))
            .flatten()
            .cloned()
            .collect()
    };
    let saved_hoists = crate::check_hoist_closure_lets::enter(checker, ast, &block);
    // A clause is entered from the head (the case values are read
    // before any body runs) and, when the clause above it in source
    // order can fall through, from that clause's end as well.
    let entry = checker.flow_entry();
    let mut above: Option<FlowState> = None;
    for (clause, body) in clause_order(cases, default.as_deref(), *default_at) {
        checker.restore_flow(&entry);
        if let Some(i) = clause {
            check_case_value(checker, ast, cases[i].value, &scrut_ty);
        }
        let start = match above.take() {
            Some(end) => checker.join_flow(&entry, &end),
            None => entry.clone(),
        };
        checker.restore_flow(&start);
        check_clause_body(checker, ast, body, clause_narrow(clause));
        if body.last().is_none_or(|s| !stmt_diverges(s)) {
            above = Some(checker.flow_at(&entry));
        }
    }
    checker.hoisted_closure_lets = saved_hoists;
    checker.scopes.pop();
    // past the switch the binding goes loose, as under any switch that
    // names it: what a returning clause leaves behind is not joined
    if let Some((name, _, _)) = &narrows {
        checker.mark_loose(name);
    }
}

/// The clauses in source order: the case index (`None` for the
/// default, an empty body when there is none) and the body.
fn clause_order<'a>(
    cases: &'a [SwitchCase],
    default: Option<&'a [Stmt]>,
    default_at: usize,
) -> Vec<(Option<usize>, &'a [Stmt])> {
    let mut order: Vec<(Option<usize>, &[Stmt])> = cases
        .iter()
        .enumerate()
        .map(|(i, c)| (Some(i), c.body.as_slice()))
        .collect();
    order.insert(default_at, (None, default.unwrap_or_default()));
    order
}

/// A case value against the scrutinee's type.
fn check_case_value(checker: &mut Checker, ast: &Ast, value: ExprId, scrut_ty: &Type) {
    match checker.type_of(ast, value) {
        // `any` on either side is the type whose runtime value is
        // unknown, so nothing can be said about it here.
        Ok(t)
            if t == *scrut_ty
                || t == Type::Any
                || *scrut_ty == Type::Any
                // A `T | undefined` scrutinee has three faces a
                // clause can equal: the T, `undefined`, and
                // `null` — the lattice spells the last two as
                // one slot but IsStrictlyEqual keeps them apart.
                // Warning "never selected" on `case undefined:`
                // under exactly the type that exists to carry
                // undefined said the opposite of the truth.
                || matches!(scrut_ty, Type::Nullable(inner)
                    if t == Type::Undefined || t == Type::Null || t == **inner)
                || nullish_clause_is_reachable(scrut_ty, &t) => {}
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
}

/// One clause body, in source order — a later clause's closures see
/// what an earlier one (the default included) declares. `narrow` is
/// the union binding this clause sees narrowed, and to what.
fn check_clause_body(
    checker: &mut Checker,
    ast: &Ast,
    body: &[Stmt],
    narrow: Option<(String, Type)>,
) {
    let saved = narrow
        .as_ref()
        .and_then(|(n, t)| checker.apply_union_narrow(n, t.clone()));
    for s in body {
        checker.check_stmt(ast, s);
    }
    if let (Some((n, _)), Some(prev)) = (&narrow, saved) {
        checker.restore_narrow(n, prev);
    }
    // ut3 — only one case body executes; no assignment narrow
    // crosses case boundaries.
    checker.flush_assign_narrows();
}

pub(crate) fn check_yield(checker: &mut Checker) {
    checker
        .errors
        .push_err("yield is only valid inside a `function*` generator body".into());
}
