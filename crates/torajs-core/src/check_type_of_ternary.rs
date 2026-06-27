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
    let c = checker.type_of(ast, cond)?;
    if !js_truthy_acceptable(&c) {
        return Err(format!(
            "ternary condition must be boolean (or coercible), got {c:?}"
        ));
    }
    let narrow = checker.collect_null_narrow(ast, cond);
    let then_saved = if let Some((name, inner, polarity)) = &narrow {
        if *polarity {
            checker.apply_narrow(name, inner.clone())
        } else {
            None
        }
    } else {
        None
    };
    let t = checker.type_of(ast, then_branch)?;
    if let (Some((name, _, _)), Some(saved)) = (&narrow, then_saved) {
        checker.restore_narrow(name, saved);
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
    let e = checker.type_of(ast, else_branch)?;
    if let (Some((name, _, _)), Some(saved)) = (&narrow, else_saved) {
        checker.restore_narrow(name, saved);
    }
    match unify_ternary(&t, &e) {
        Some(ty) => Ok(ty),
        None => Err(format!(
            "ternary branches differ — `then` is {t:?}, `else` is {e:?}"
        )),
    }
}
