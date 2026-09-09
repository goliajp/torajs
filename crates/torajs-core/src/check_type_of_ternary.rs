//! `Expr::Ternary { cond, then_branch, else_branch }` typecheck
//! pulled out of [`crate::check::Checker::type_of_inner`]'s
//! `Expr::Ternary` arm as chunk-97 of the type_of_inner decomp.
//!
//! 3-step process:
//!
//! 1. **Condition typecheck** — must satisfy `js_truthy_acceptable`
//!    (Boolean or any truthy-coercible type per ES §13.13).
//! 2. **V3-18 ternary-narrow wedge** — mirror if-stmt null-narrow
//!    so `cond ? then : else` works for the canonical TS pattern
//!    `s ? s.length : 0`. `collect_null_narrow` finds an
//!    `Ident-null` shape in the condition; polarity gates whether
//!    then- or else-branch gets the narrowed inner type
//!    (`apply_narrow` returns the saved prev type, `restore_narrow`
//!    after typing the branch).
//! 3. **Branch unification** — `unify_ternary` widens one side to
//!    `Nullable<T>` when the other is `T` or `Null`. Common pattern
//!    with optional params: `x === null ? default : x` where
//!    then=T and else=Nullable<T>.

use crate::ast::{Ast, ExprId};
use crate::check::{Checker, Type, js_truthy_acceptable, unify_ternary};

pub(crate) fn check(
    checker: &mut Checker,
    ast: &Ast,
    cond: ExprId,
    then_branch: ExprId,
    else_branch: ExprId,
) -> Result<Type, String> {
    // Every one of the three operands is VISITED before any verdict
    // propagates. `type_of` records into `expr_types` only on Ok, so
    // a `?` here left the two branches with NO recorded type — and in
    // a monomorph specialization body (diagnostics discarded by
    // design) lowering reads exactly that table. `(a - b) ? (o.q ===
    // undefined) : "E"` answered FALSE because the `undefined`
    // literal had no entry and strict-eq packed the null tag.
    // Same shape as `check_type_of_binop::check`.
    let cond_verdict = match checker.type_of(ast, cond) {
        Ok(c) if js_truthy_acceptable(&c) => Ok(c),
        Ok(c) => Err(format!(
            "ternary condition must be boolean (or coercible), got {c:?}"
        )),
        Err(e) => Err(e),
    };
    let narrow = checker.collect_null_narrow(ast, cond);
    // Chunk 782 — member-path mirror (`o.s ? o.s.length : -1`): the
    // if-stmt wedge (RFC 20260710 C5) applied to the expression
    // form, same polarity dance as the binding narrow.
    let member_narrow = checker.collect_member_narrow(ast, cond);
    let then_saved = if let Some((name, inner, polarity)) = &narrow {
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
    // The restore below has to run even when the branch fails to
    // type: an unbalanced `apply_narrow` leaves the binding narrowed
    // for the REST of the check, which is checker state corruption on
    // an error path that no diagnostic reports.
    let t = checker.type_of(ast, then_branch);
    if let (Some((name, _, _)), Some(saved)) = (&narrow, then_saved) {
        checker.restore_narrow(name, saved);
    }
    if let (Some((key, _, _)), Some(prev)) = (&member_narrow, then_member) {
        checker.restore_member_narrow(key, prev);
    }
    let else_saved = if let Some((name, inner, polarity)) = &narrow {
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
    let e = checker.type_of(ast, else_branch);
    if let (Some((name, _, _)), Some(saved)) = (&narrow, else_saved) {
        checker.restore_narrow(name, saved);
    }
    if let (Some((key, _, _)), Some(prev)) = (&member_narrow, else_member) {
        checker.restore_member_narrow(key, prev);
    }
    cond_verdict?;
    let (t, e) = (t?, e?);
    // 506-02 — two DIFFERENT class instances join to their nearest
    // common ancestor (`cond ? new Mid(i) : new Leaf(i)` types
    // ClassRef("Mid")); the lowering keeps both pointers unboxed on
    // the ancestor's Obj repr, the same layout-prefix invariant
    // `let b: Base = new Leaf()` rides, so every consumer dispatches
    // on the static ancestor. Two user classes with no common
    // ancestor join to ANY, the S129-1 posture (both branches box,
    // consumers read the any lanes) — bun runs the program either
    // way. Builtin-backed refs keep the reject below.
    if let Some(joined) = crate::check_type_compare::class_join(ast, &t, &e) {
        return Ok(joined);
    }
    match unify_ternary(&t, &e) {
        Some(ty) => Ok(ty),
        None => Err(format!(
            "ternary branches differ — `then` is {t:?}, `else` is {e:?}"
        )),
    }
}
