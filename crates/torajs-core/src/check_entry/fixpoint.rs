//! The checker's full pipeline, run to its fixpoint.

use std::collections::HashSet;

use crate::ast::{Ast, ExprId};
use crate::check::{Checker, Severity};

/// The full pipeline to its fixpoint ([`fixpoint_with`]).
///
/// A call the any-widen lane types `any` (`widen_ret_any`) is still
/// typed by its declared return in TS: `const k: number = echo(v)`
/// against `echo(s: string): string` is a type error, and the spec
/// gives it no run-time meaning. A second run types those calls by
/// the declared return; its errors are the program's. A program it
/// accepts lowers with the `any` faces, unless some position takes the
/// declared `string` but not `any` (`for (p of echo(v).split(","))`):
/// then it lowers from the declared run, as if no call were typed
/// `any`.
pub(super) fn check_to_fixpoint(ast: &Ast) -> Checker {
    fixpoint_hidden(ast, &HashSet::new())
}

/// [`check_to_fixpoint`] with the call arguments `hidden` holds read as
/// values of any kind ([`super::hidden_any::settle`]).
pub(super) fn fixpoint_hidden(ast: &Ast, hidden: &HashSet<ExprId>) -> Checker {
    let c = fixpoint_with(ast, false, hidden);
    if c.widen_ret_any.is_empty() {
        return c;
    }
    let declared = fixpoint_with(ast, true, hidden);
    if has_errors(&c) || has_errors(&declared) {
        declared
    } else {
        c
    }
}

fn has_errors(c: &Checker) -> bool {
    c.errors.iter().any(|d| d.severity == Severity::Error)
}

/// The full pipeline, re-run until the cross-type widen set stops
/// growing (RFC 20260927-inferred-slot-any-widen). A binding assigned
/// an `any` value types `any` from its declaration, which changes the
/// types of every read of it — the whole table has to be recomputed
/// with the binding widened, and that can widen the next one
/// (`u = total`). The set only grows and is bounded by the program's
/// mutable `let`s.
///
/// The property / return slot census (`check::face_slots`) and the
/// binding and parameter census (`Checker::face_mismatch_lets` /
/// `face_mismatch_params`) are carried into the next run the same way,
/// when a read asked for one of their keys before the store that
/// entered it (`Reads::stale`): that call is typed again once the
/// census holds it. They too only grow.
fn fixpoint_with(ast: &Ast, widen_ret_declared: bool, hidden: &HashSet<ExprId>) -> Checker {
    let mut c = Checker::new();
    c.widen_ret_declared = widen_ret_declared;
    c.hidden_any_args = hidden.clone();
    c.run_full_pipeline(ast);
    loop {
        let found = crate::let_widen::collect_cross_type_widen_inits(ast, Some(&c.expr_types));
        if found.is_subset(&c.cross_type_widened) && !c.face_slot_reads.stale {
            return c;
        }
        let mut seed = std::mem::take(&mut c.cross_type_widened);
        seed.extend(found);
        let face_slots = std::mem::take(&mut c.face_slots);
        let lets = std::mem::take(&mut c.face_mismatch_lets);
        let params = std::mem::take(&mut c.face_mismatch_params);
        c = Checker::new();
        c.widen_ret_declared = widen_ret_declared;
        c.hidden_any_args = hidden.clone();
        c.cross_type_widened = seed;
        c.face_slots = face_slots;
        c.face_mismatch_lets = lets;
        c.face_mismatch_params = params;
        c.run_full_pipeline(ast);
    }
}
