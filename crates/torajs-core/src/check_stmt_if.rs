//! `Stmt::If { cond, then_branch, else_branch }` typecheck pulled
//! out of [`crate::check::Checker::check_stmt`]'s `Stmt::If` arm as
//! chunk-101 of the check_stmt decomp.
//!
//! Three V3-18 narrowing wedges + CFG-aware moved tracking:
//!
//! 1. **Condition** — `js_truthy_acceptable` (Boolean or coercible
//!    per §13.13); error otherwise.
//! 2. **In-branch narrowing** — `collect_null_narrow` finds an
//!    `Ident-null` shape in the condition. Polarity true (`!== null`)
//!    narrows in the then-branch; polarity false (`=== null`) narrows
//!    in the else-branch. `apply_narrow` returns the saved prev type,
//!    `restore_narrow` after typing the branch (so nested ifs
//!    compose correctly). A branch that writes the binding straight-
//!    line opens the guard ledger (`check_assigns_to`), so the write
//!    is checked against the declared `T | null` and retires the
//!    narrow where it happens.
//! 3. **CFG-aware moved snapshot** — `snapshot_moved` before each
//!    branch, run branch, capture post-state, `restore_moved` so
//!    each branch starts fresh. `join_branch_moves` joins post-
//!    states: a binding is moved post-if iff every non-diverging
//!    branch consumed it. This makes `if (cond) return f; return f;`
//!    work (then diverges so its consume of `f` doesn't propagate).
//!    Member narrows (`if (t.next !== null)`) are joined the same way:
//!    each branch starts from the state before the `if`, and a narrow
//!    a branch's write retired stays retired after it.
//! 4. **Post-if narrowing** — the binding is narrowed after the `if`
//!    when every branch that falls through ends with it non-null:
//!    the narrowed branch unless it wrote a possibly-null value, the
//!    other one if it assigned a non-null value (`if (a === null)
//!    a = new A()`), an absent else when the condition's false side
//!    is the narrowed one (`if (a === null) return`).

use crate::ast::{Ast, ExprId, Stmt};
use crate::check::{Checker, DiagPush, MovedSnapshot, Type, js_truthy_acceptable, stmt_diverges};
use crate::check_assigns_to::{Guarded, Region, binding_hit, scan};
use crate::check_stmt_while::member_region;

/// A null narrow collected from a condition: binding, inner type,
/// whether the then-branch is the narrowed one.
type Narrow = Option<(String, Type, bool)>;

/// Open the guard ledger for the branch `narrow` applies to (`on`
/// names which branch that is) when the branch writes the binding
/// straight-line.
fn open_branch_guard(
    checker: &mut Checker,
    ast: &Ast,
    narrow: &Narrow,
    branch: &Stmt,
    on: bool,
) -> crate::check::GuardSave {
    let (name, inner, polarity) = narrow.as_ref()?;
    let hit = binding_hit(ast, name);
    if *polarity != on
        || scan(
            ast,
            branch,
            &Guarded {
                root: name,
                hit: &hit,
            },
        ) != Region::StraightLine
    {
        return None;
    }
    checker.open_guard(name, Type::Nullable(Box::new(inner.clone())))
}

/// Put the member narrow a branch installed back to what it displaced,
/// unless a write in the branch retired it: the receiver or the member
/// may then hold anything after the branch.
fn settle_member_narrow(checker: &mut Checker, key: &(String, String), prev: Option<Type>) {
    if checker.narrows.member.contains_key(key) {
        checker.restore_member_narrow(key, prev);
    }
}

/// Does the narrowed binding hold a non-null value at this point of
/// the walk?
fn holds(checker: &Checker, narrow: &Narrow) -> bool {
    narrow.as_ref().is_some_and(|(n, _, _)| {
        checker
            .lookup(n)
            .is_some_and(|i| !matches!(i.ty, Type::Nullable(_)))
    })
}

pub(crate) fn check(
    checker: &mut Checker,
    ast: &Ast,
    cond: ExprId,
    then_branch: &Stmt,
    else_branch: &Option<Box<Stmt>>,
) {
    match checker.type_of(ast, cond) {
        Ok(t) if js_truthy_acceptable(&t) => {}
        Ok(other) => checker.errors.push_err(format!(
            "if condition must be boolean (or coercible), got {other:?}"
        )),
        Err(e) => checker.errors.push_err(e),
    }
    let narrow = checker.collect_null_narrow(ast, cond);
    // RFC 20260710 C5 — member-path guard (`if (o.cb) { o.cb() }`):
    // parallels the binding narrow; the entry lives in
    // `narrows.member` (assignments to the receiver or the member
    // invalidate it, see check_type_of_assign).
    let member_narrow = checker.collect_member_narrow(ast, cond);
    // `typeof x === "tag"` on a union binding: each branch sees its
    // members (the narrowed type is still a union, see union_narrow)
    let union_narrow = checker.union_guard(ast, cond);
    let pre = checker.snapshot_moved();
    let guard_pre = checker.guard_snapshot();
    let member_pre = checker.narrows.member.clone();
    // narrowed on the way in by more than a straight-line assignment
    // (which the flush after the `if` retires anyway)
    let narrowed_before = holds(checker, &narrow)
        && !narrow
            .as_ref()
            .is_some_and(|n| checker.narrows.assign.contains_key(&n.0));
    let then_union = union_narrow
        .as_ref()
        .and_then(|(n, t, _)| checker.apply_union_narrow(n, t.clone()));
    let then_narrow = if let Some((name, inner, polarity)) = &narrow {
        if *polarity {
            checker.apply_narrow(name, inner.clone())
        } else {
            None
        }
    } else {
        None
    };
    let then_guard = open_branch_guard(checker, ast, &narrow, then_branch, true);
    let then_member = if let Some((key, inner, polarity)) = &member_narrow {
        if *polarity && member_region(ast, then_branch, key) != Region::Unsafe {
            Some(checker.apply_member_narrow(key, inner.clone()))
        } else {
            None
        }
    } else {
        None
    };
    checker.check_stmt(ast, then_branch);
    let then_holds = holds(checker, &narrow);
    if let (Some((name, _, _)), Some(saved)) = (&union_narrow, then_union) {
        checker.restore_narrow(name, saved);
    }
    // The guard's restore runs FIRST, then the flush. `saved` is the
    // type the binding had before this guard, which may itself be a
    // straight-line assign narrow — and the flush exists precisely to
    // retire those. Restoring after it resurrected one: `q = {x: 1};
    // if (q !== null) { … } q = null` refused the last line, because
    // `q` had been put back to the narrowed `Struct` after the flush
    // had already returned it to the union.
    if let (Some((name, _, _)), Some(saved)) = (&narrow, then_narrow) {
        checker.restore_narrow(name, saved);
    }
    checker.close_guard(then_guard);
    // ut3 — assignment narrows minted inside the then-branch must
    // not leak into the else walk (only one branch executes).
    checker.flush_assign_narrows();
    if let (Some((key, _, _)), Some(prev)) = (&member_narrow, then_member) {
        settle_member_narrow(checker, key, prev);
    }
    let then_members = std::mem::replace(&mut checker.narrows.member, member_pre.clone());
    let then_guards = checker.guard_snapshot();
    checker.guard_restore(&guard_pre);
    let then_div = stmt_diverges(then_branch);
    let then_post = checker.snapshot_moved();
    checker.restore_moved(&pre);
    let mut else_guards = guard_pre.clone();
    let mut else_holds = narrow.as_ref().is_some_and(|(_, _, polarity)| !*polarity);
    let (else_div, else_post): (bool, Option<MovedSnapshot>) = if let Some(eb) = else_branch {
        let else_narrow = if let Some((name, inner, polarity)) = &narrow {
            if !*polarity {
                checker.apply_narrow(name, inner.clone())
            } else {
                None
            }
        } else {
            None
        };
        let else_guard = open_branch_guard(checker, ast, &narrow, eb, false);
        let else_member = if let Some((key, inner, polarity)) = &member_narrow {
            if !*polarity && member_region(ast, eb, key) != Region::Unsafe {
                Some(checker.apply_member_narrow(key, inner.clone()))
            } else {
                None
            }
        } else {
            None
        };
        let else_union = union_narrow
            .as_ref()
            .and_then(|(n, _, e)| checker.apply_union_narrow(n, e.clone()));
        checker.check_stmt(ast, eb);
        else_holds = holds(checker, &narrow);
        if let (Some((name, _, _)), Some(saved)) = (&union_narrow, else_union) {
            checker.restore_narrow(name, saved);
        }
        if let (Some((name, _, _)), Some(saved)) = (&narrow, else_narrow) {
            checker.restore_narrow(name, saved);
        }
        checker.close_guard(else_guard);
        checker.flush_assign_narrows();
        if let (Some((key, _, _)), Some(prev)) = (&member_narrow, else_member) {
            settle_member_narrow(checker, key, prev);
        }
        else_guards = checker.guard_snapshot();
        let div = stmt_diverges(eb);
        let snap2 = checker.snapshot_moved();
        checker.restore_moved(&pre);
        (div, Some(snap2))
    } else {
        (false, None)
    };
    checker.join_branch_moves(&pre, &then_post, then_div, else_post.as_deref(), else_div);
    // a write in a branch that cannot fall through retires nothing
    // after the `if` (`if (k) { q = q.next; continue } use(q)`)
    let guards = match (then_div, else_div) {
        (true, true) => guard_pre,
        (true, false) => else_guards,
        (false, true) => then_guards,
        (false, false) => checker.guard_join(&then_guards, &else_guards),
    };
    checker.guard_restore(&guards);
    let else_members = std::mem::take(&mut checker.narrows.member);
    checker.narrows.member = match (then_div, else_div) {
        (true, true) => member_pre,
        (true, false) => else_members,
        (false, true) => then_members,
        (false, false) => then_members
            .into_iter()
            .filter(|(k, t)| else_members.get(k) == Some(t))
            .collect(),
    };
    // `if (o.f === null) return` with no else: the code after the `if`
    // runs only where the test was false, as a binding's narrow does
    if let Some((key, inner, false)) = &member_narrow
        && then_div
        && else_branch.is_none()
    {
        checker.apply_member_narrow(key, inner.clone());
    }
    if let Some((name, inner, _)) = &narrow {
        let falls_through = !(then_div && else_div);
        if falls_through && (then_div || then_holds) && (else_div || else_holds) {
            checker.apply_narrow(name, inner.clone());
            // both branches fall through non-null: a narrow only an exit
            // used to mint, and a later write may retire this one
            if !then_div && !else_div && !narrowed_before && checker.guard_declared(name).is_none()
            {
                checker.set_flow_declared(name, Type::Nullable(Box::new(inner.clone())));
            }
        } else if (!then_div && !then_holds) || (else_branch.is_some() && !else_div && !else_holds)
        {
            // a branch that falls through left the binding possibly
            // null, whatever narrow it entered the `if` with
            checker.apply_narrow(name, Type::Nullable(Box::new(inner.clone())));
        }
    }
    if let Some((name, t, e)) = &union_narrow {
        use crate::check::End;
        let then_end = checker.stmt_end(ast, then_branch);
        let else_end = else_branch
            .as_deref()
            .map_or(End::Completes, |eb| checker.stmt_end(ast, eb));
        match (then_end, else_end) {
            (End::Diverges, End::Completes) => {
                checker.apply_union_narrow(name, e.clone());
            }
            (End::Completes, End::Diverges) => {
                checker.apply_union_narrow(name, t.clone());
            }
            // a branch that may diverge in a form not decided here
            // leaves the members after the `if` unknown
            (End::Unknown, _) | (_, End::Unknown) => checker.mark_loose(name),
            _ => {}
        }
    }
}
