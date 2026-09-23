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
//! Extra trailing args used to be a third group here, one wedge for
//! Math / Date receivers and one for a bare-Ident or IIFE callee.
//! [`drop_extra_args`] below says the same thing for every callee and
//! runs right after this chain, so both were doing its work over a
//! subset; they are gone.

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
    Ok(())
}

/// RFC 20260810-indirect-argc-abi S3.5 — beyond-arity calls are
/// universally admitted (ES §13.3.6.1: every argument evaluates,
/// extras never bind a parameter). The extras still typecheck —
/// a type error inside one stays loud — then drop out of the
/// pairing; the lowering lanes evaluate them for their side
/// effects without letting them enter argv, and argc-carrying
/// ABIs pass the REAL count. Replaces the name-keyed length-only
/// closure wedge (check_type_of_call_closure_argc): the general
/// rule covers its list.
///
/// It is also why the two trailing-ignore wedges that used to sit at
/// the end of [`apply`] are gone: each typed the extras and truncated
/// exactly as this does, over a subset of the callees, one line
/// earlier.
pub(super) fn drop_extra_args(
    checker: &mut Checker,
    ast: &Ast,
    effective_args: &mut Vec<ExprId>,
    params: &[Type],
) -> Result<(), String> {
    if effective_args.len() > params.len() {
        for a in &effective_args[params.len()..] {
            checker.type_of(ast, *a)?;
        }
        effective_args.truncate(params.len());
    }
    Ok(())
}

/// §7.2 middle tier at a callback slot the call left EMPTY.
///
/// §18 binds a missing argument to undefined, and the method's own
/// clause then says "If IsCallable(callbackfn) is false, throw a
/// TypeError" — a runtime answer, so refusing `[].flatMap()` would be
/// the static-subset one. Answers that throw's message, having typed
/// the arguments the call DID supply so a type error inside one stays
/// loud.
///
/// The predicate is [`super::callback_slot_throw::message_for`],
/// handed the undefined §18 supplies: it is the one place that knows
/// which receivers and which method names carry such a clause, and
/// that `sort` / `toSorted` read a missing comparator as "no
/// comparator" instead. Missing arguments are always trailing, so the
/// first unpaired param is the slot in question.
pub(super) fn missing_callback_throw(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    params: &[Type],
    effective_args: &[ExprId],
) -> Result<Option<String>, String> {
    if effective_args.len() >= params.len() {
        return Ok(None);
    }
    let Some(msg) = super::callback_slot_throw::message_for(
        checker,
        ast,
        *callee,
        &params[effective_args.len()],
        &Type::Undefined,
    ) else {
        return Ok(None);
    };
    for a in effective_args {
        checker.type_of(ast, *a)?;
    }
    Ok(Some(msg))
}
