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

mod any_widen;
mod general;
mod route_arity_widen;
mod route_collections;
mod route_early;
mod route_globals;
mod route_namespace_trailing;
mod route_str_arr_trailing;

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

pub(crate) fn check(
    checker: &mut Checker,
    ast: &Ast,
    eid: ExprId,
    callee: &ExprId,
    args: &Vec<ExprId>,
) -> Result<Type, String> {
    // RFC 20260806 blade 2 — the member-read gate in
    // `check_type_of_member` only sees calls whose callee gets typed on
    // the way past; the routes below claim many builtin calls straight
    // from the callee's syntax, so a patched method reached that way
    // never met the gate. Answering here covers them, and recording the
    // callee as Any is what makes the lowering side notice: its
    // cluster-#4 branch keys on exactly that record.
    if !checker.proto_shadow.is_empty()
        && let Expr::Member { obj, name } = ast.get_expr(*callee)
        && let Ok(obj_ty) = checker.type_of(ast, *obj)
        && let Some(family) = crate::builtin_proto_shadow::family_of(&obj_ty)
        && checker.proto_shadow.shadows(family, name)
    {
        // Type the arguments before standing down. Returning straight
        // from here skips the walk every other route performs, and
        // that walk is not just about the answer — checking an
        // argument is what records the call sites an implicit generic
        // is instantiated from. Without it a bare `function
        // callbackfn(val, idx, obj)` handed to a patched
        // `Array.prototype.every` is never specialized, and lowering
        // rejects the whole program with "unknown function". The
        // result is deliberately discarded: an argument that does not
        // type is the general path's error to report, not this
        // gate's.
        for &a in args {
            let _ = checker.type_of(ast, a);
        }
        checker.expr_types.insert(*callee, Type::Any);
        return Ok(Type::Any);
    }
    // Rotation 372 — a call whose argument list still carries a
    // dynamic `...spread` (it survived every static expander) types
    // as Any and lowers through the runtime spread kernels
    // (§13.3.8.1 ArgumentListEvaluation — argc is a runtime fact).
    // Claimed before every route: a fixed-arity route would read the
    // spread as ONE argument. Spread sources and literal args both
    // walk (the walk records implicit-generic instantiation sites);
    // iterability of a source is §7.4.2 GetIterator's runtime
    // question, and callee callability is the any lane's runtime
    // TypeError — so the callee walk is deliberately lenient.
    if args
        .iter()
        .any(|a| matches!(ast.get_expr(*a), Expr::Spread { .. }))
    {
        for &a in args {
            match ast.get_expr(a) {
                Expr::Spread { expr } => {
                    checker.type_of(ast, *expr)?;
                }
                _ => {
                    checker.type_of(ast, a)?;
                }
            }
        }
        match ast.get_expr(*callee) {
            Expr::Member { obj, .. } => {
                let _ = checker.type_of(ast, *obj);
            }
            _ => {
                let _ = checker.type_of(ast, *callee);
            }
        }
        return Ok(Type::Any);
    }
    // T-12 tagged templates — the synthetic template-object call the
    // parser desugar plants: every argument is a compile-time literal
    // (site number + cooked/raw string pairs), the answer is the
    // runtime template-object cell. Claimed by exact callee spelling
    // before every route.
    if let Expr::Ident(n) = ast.get_expr(*callee)
        && n == crate::parser::TEMPLATE_OBJECT_CALLEE
    {
        return Ok(Type::Any);
    }
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
    // 398-10 — an Array-receiver HOF whose callback types `any` routes
    // to the runtime any-method lane (after every narrower route so
    // the verified typed lanes keep their claims; before the general
    // tail whose per-arg loop rejects the Any callback).
    if let Some(r) = crate::check_type_of_call_arr_hof_any_cb::try_match(checker, ast, callee, args)
    {
        return r;
    }
    general::general_call(checker, ast, eid, callee, args)
}
