//! `Expr::Call` typecheck extracted from
//! [`crate::check::Checker::type_of_inner`]'s `Expr::Call` arm
//! (chunk 168), decomposed into cascade segments (chunk 433 —
//! RFC `20260703-check-type-of-call-decomp`).
//!
//! Pre-extract this arm was 4431 LOC inside `type_of_inner` — the
//! largest single arm in the type checker. Chunks 207-311 moved
//! each wedge's decision body into a flat
//! `crate::check_type_of_call_*` sibling; what remained here was
//! the 1071-line cascade dispatcher itself. That cascade is now
//! split into six consecutive segments plus the general fn-call
//! typing tail, one submodule each under `check_type_of_call/`.
//!
//! INVARIANT: the early-route cascade is order-sensitive (several
//! arms must run BEFORE the regular method-table dispatch, and
//! earlier permissive arms shadow later narrow ones — see chunk
//! 220's dead-code removal). Segment boundaries are mechanical
//! (consecutive), NOT semantic regroupings; the relative order of
//! every arm is preserved verbatim. When adding a new wedge, pick
//! the segment by cascade position, never by receiver family.

mod general;
mod route_arity_widen;
mod route_collections;
mod route_early;
mod route_globals;
mod route_namespace_trailing;
mod route_str_arr_trailing;

use crate::ast::{Ast, ExprId};
use crate::check::{Checker, Type};

pub(crate) fn check(
    checker: &mut Checker,
    ast: &Ast,
    eid: ExprId,
    callee: &ExprId,
    args: &Vec<ExprId>,
) -> Result<Type, String> {
    if let Some(r) = route_early::try_route(checker, ast, eid, callee, args) {
        return r;
    }
    if let Some(r) = route_globals::try_route(checker, ast, eid, callee, args) {
        return r;
    }
    if let Some(r) = route_arity_widen::try_route(checker, ast, callee, args) {
        return r;
    }
    if let Some(r) = route_str_arr_trailing::try_route(checker, ast, callee, args) {
        return r;
    }
    if let Some(r) = route_namespace_trailing::try_route(checker, ast, callee, args) {
        return r;
    }
    if let Some(r) = route_collections::try_route(checker, ast, callee, args) {
        return r;
    }
    general::general_call(checker, ast, eid, callee, args)
}
