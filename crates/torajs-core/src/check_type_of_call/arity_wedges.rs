//! The post-T-28 arity-shaping wedge chain, lifted out of
//! [`super::general`] verbatim (rotation 697; that file was at 487
//! prod lines against the 500 limit and the next rule needs room).
//!
//! Two different jobs are stacked here and the file-size move is a
//! good moment to name them, because the chain reads as one list:
//!
//! - **Missing trailing args** — `date_setter_narrow`,
//!   `array_splice_narrow`, `array_at_narrow`, `search_0arg`. Each
//!   trims `params` so a call that left a slot out still pairs, and
//!   each has a lowering site that fills the slot's spec default.
//!   The Date setters and splice are there because the spec asks
//!   whether the argument is *present*, which is not the same
//!   question as whether it is undefined (§21.4.4.22,
//!   §23.1.3.31) — those two cannot become a general rule.
//! - **Explicit undefined** — `array_splice_2arg_undef` rewrites the
//!   two-argument shape, which for splice means deleteCount 0 rather
//!   than "to the end". It is in neither of the other groups.
//! - **Extra trailing args** — `math_date_trailing_ignore` and
//!   `direct_trailing_ignore`. RFC 20260810 S3.5 made that universal
//!   in `general_call` itself, ABOVE the arity gate, so by the time
//!   control reaches this chain there are no extra args left for
//!   either to see. Kept for one more rotation so their removal is
//!   its own commit with its own gate.

use crate::ast::{Ast, ExprId};
use crate::check::{Checker, Type};

/// Post-T-28 arity-shaping wedge chain — the per-method narrows
/// (Date setters / splice / at / search-0arg) plus the trailing-arg
/// ignore pair (Math-Date receivers, then the direct bare-Ident /
/// IIFE callee). All same-shape `apply(..)?` calls with no early
/// return; each wedge's contract lives in its own module doc.
pub(super) fn apply(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    effective_args: &mut Vec<ExprId>,
    params: &mut Vec<Type>,
) -> Result<(), String> {
    // Date per-field setter arity narrow wedge (chunk 299).
    crate::check_type_of_call_date_setter_narrow::apply(
        checker,
        ast,
        callee,
        effective_args,
        params,
    )?;
    // `arr.splice` / `arr.toSpliced` arity narrow wedge (chunk 300).
    crate::check_type_of_call_array_splice_narrow::apply(
        checker,
        ast,
        callee,
        effective_args,
        params,
    )?;
    // S237 splice/toSpliced 2-arg-undef arity narrow wedge (chunk 301).
    crate::check_type_of_call_array_splice_2arg_undef::apply(
        checker,
        ast,
        callee,
        effective_args,
        params,
    )?;
    // `arr.at` / `s.at` 0-arg arity narrow wedge (chunk 302).
    crate::check_type_of_call_array_at_narrow::apply(checker, ast, callee, effective_args, params)?;
    // Array/String search-method 0-arg arity narrow wedge (chunk 303).
    crate::check_type_of_call_search_0arg::apply(checker, ast, callee, effective_args, params)?;
    // S243 / S250 — Math.* / Date.<static> trailing-arg ignore wedge
    // (chunk 304).
    crate::check_type_of_call_math_date_trailing_ignore::apply(
        checker,
        ast,
        callee,
        effective_args,
        params,
    )?;
    // Direct-call (bare-Ident / IIFE callee) trailing-arg ignore
    // wedge — ES §13.3.6.1 evaluates every arg, the callee binds
    // only its declared formals.
    crate::check_type_of_call_direct_trailing_ignore::apply(
        checker,
        ast,
        callee,
        effective_args,
        params,
    )?;
    Ok(())
}
