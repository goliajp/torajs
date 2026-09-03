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
//!    compose correctly).
//! 3. **CFG-aware moved snapshot** — `snapshot_moved` before each
//!    branch, run branch, capture post-state, `restore_moved` so
//!    each branch starts fresh. `join_branch_moves` joins post-
//!    states: a binding is moved post-if iff every non-diverging
//!    branch consumed it. This makes `if (cond) return f; return f;`
//!    work (then diverges so its consume of `f` doesn't propagate).
//! 4. **Post-if narrowing on diverge** — if one branch diverges
//!    (early return/throw/break/continue), the other state
//!    propagates out. Polarity true + else diverges → post-if
//!    narrows to inner; polarity false + then diverges → post-if
//!    narrows to inner.

use crate::ast::{Ast, ExprId, Stmt};
use crate::check::{Checker, DiagPush, MovedSnapshot, js_truthy_acceptable, stmt_diverges};

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
    // `member_narrows` (assignments to the receiver or the member
    // invalidate it, see check_type_of_assign).
    let member_narrow = checker.collect_member_narrow(ast, cond);
    let pre = checker.snapshot_moved();
    let then_narrow = if let Some((name, inner, polarity)) = &narrow {
        if *polarity {
            checker.apply_narrow(name, inner.clone())
        } else {
            None
        }
    } else {
        None
    };
    let then_member = if let Some((key, inner, polarity)) = &member_narrow {
        if *polarity {
            Some(checker.apply_member_narrow(key, inner.clone()))
        } else {
            None
        }
    } else {
        None
    };
    checker.check_stmt(ast, then_branch);
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
    // ut3 — assignment narrows minted inside the then-branch must
    // not leak into the else walk (only one branch executes).
    checker.flush_assign_narrows();
    if let (Some((key, _, _)), Some(prev)) = (&member_narrow, then_member) {
        checker.restore_member_narrow(key, prev);
    }
    let then_div = stmt_diverges(then_branch);
    let then_post = checker.snapshot_moved();
    checker.restore_moved(&pre);
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
        let else_member = if let Some((key, inner, polarity)) = &member_narrow {
            if !*polarity {
                Some(checker.apply_member_narrow(key, inner.clone()))
            } else {
                None
            }
        } else {
            None
        };
        checker.check_stmt(ast, eb);
        if let (Some((name, _, _)), Some(saved)) = (&narrow, else_narrow) {
            checker.restore_narrow(name, saved);
        }
        checker.flush_assign_narrows();
        if let (Some((key, _, _)), Some(prev)) = (&member_narrow, else_member) {
            checker.restore_member_narrow(key, prev);
        }
        let div = stmt_diverges(eb);
        let snap2 = checker.snapshot_moved();
        checker.restore_moved(&pre);
        (div, Some(snap2))
    } else {
        (false, None)
    };
    checker.join_branch_moves(&pre, &then_post, then_div, else_post.as_deref(), else_div);
    if let Some((name, inner, polarity)) = &narrow {
        let post_narrow_to_inner = (*polarity && else_div) || (!*polarity && then_div);
        if post_narrow_to_inner {
            checker.apply_narrow(name, inner.clone());
        }
    }
}
