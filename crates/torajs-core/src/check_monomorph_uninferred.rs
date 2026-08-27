//! Which generic calls in a freshly-checked specialization body got
//! no inference record — and therefore no monomorph retarget.
//!
//! [`crate::check_monomorph`] checks each specialization body for
//! INFERENCE and discards the diagnostics it raises: those bodies
//! were never checked before that pass existed, and the checker is
//! stricter than the lowerer on two known shapes (TypeVars left
//! inside fn-type param anns, closure-argc ABI arity). Discarding is
//! right for a diagnostic — the call still got its record and the
//! program still lowers.
//!
//! It is NOT right when the failure is what left a HOLE. A generic
//! call the checker could not infer records nothing, so nothing
//! rewrites its callee, and `ssa_lower_resolve_callee` reaches a
//! generic decl that lower pass-1 skipped and panics `unknown
//! function \`sv\`` — naming a function that is perfectly well known
//! and saying nothing about the actual problem. The same
//! non-unifying call written at top level answers loudly and
//! precisely ("type parameter `T` was inferred as Undefined earlier
//! but here is Number"): one question, two answers, and the worse
//! one wins wherever a specialization body is involved.
//!
//! So the body's diagnostics survive exactly when it left a hole.
//! No program that lowers today changes: a body with every call
//! recorded still discards, and a body with a hole is one the
//! lowerer was going to panic on.
//!
//! The scan needs no walker. `clone_spec_body` hands back the
//! old→new `id_map`, and the clone's ExprIds ARE its second column —
//! so the body's expressions are enumerable directly off the arena.

use std::collections::HashMap;

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Diagnostic, Type};
use crate::ssa_lower_generics_monomorph::Generics;

/// Names of generic callees this specialization body calls that have
/// no `generic_call_sites` record. Deduped, in first-seen order.
pub(crate) fn uninferred_callees(
    ast: &Ast,
    id_map: &[(ExprId, ExprId)],
    generics: &Generics,
    recorded: &HashMap<ExprId, (String, Vec<Type>)>,
) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for &(_, new) in id_map {
        let Expr::Call { callee, .. } = ast.get_expr(new) else {
            continue;
        };
        let Expr::Ident(name) = ast.get_expr(*callee) else {
            continue;
        };
        // Only the generics the mono pass owns: those are exactly the
        // decls lower pass-1 skips, so a missing retarget on one of
        // them is a guaranteed panic. Every other callee resolves by
        // name like any direct call.
        if !generics.contains_key(name) {
            continue;
        }
        if recorded.contains_key(&new) {
            continue;
        }
        if !out.iter().any(|n| n == name) {
            out.push(name.clone());
        }
    }
    out
}

/// Settle one specialization body's diagnostics: discard them, which
/// is the default and was the only behaviour, unless the body left a
/// hole. A hole keeps whatever the body's own check said — which for
/// the shape that produces holes is the precise message the SAME call
/// written at top level answers with. When the check said nothing (it
/// never reached the call), name the hole instead of letting the
/// lowerer name the callee.
pub(crate) fn settle_body_diagnostics(
    c: &mut Checker,
    ast: &Ast,
    id_map: &[(ExprId, ExprId)],
    generics: &Generics,
    recorded: &HashMap<ExprId, (String, Vec<Type>)>,
    saved_error_count: usize,
    spec_fn_name: &str,
) {
    let holes = uninferred_callees(ast, id_map, generics, recorded);
    if holes.is_empty() {
        c.errors.truncate(saved_error_count);
        return;
    }
    if c.errors.len() != saved_error_count {
        return;
    }
    for callee in &holes {
        c.errors.push(Diagnostic::error(format!(
            "cannot infer type arguments for the call to `{callee}` inside `{spec_fn_name}`"
        )));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::Ast;

    fn call_of(ast: &mut Ast, callee: &str) -> ExprId {
        let c = ast.add_expr(Expr::Ident(callee.into()));
        ast.add_expr(Expr::Call {
            callee: c,
            args: Vec::new(),
        })
    }

    fn generics_with(name: &str) -> Generics {
        let mut g: Generics = HashMap::new();
        g.insert(name.into(), (Vec::new(), Vec::new(), None, Vec::new()));
        g
    }

    #[test]
    fn a_recorded_generic_call_is_not_a_hole() {
        let mut ast = Ast::default();
        let eid = call_of(&mut ast, "sv");
        let mut recorded: HashMap<ExprId, (String, Vec<Type>)> = HashMap::new();
        recorded.insert(eid, ("sv".into(), Vec::new()));
        let got = uninferred_callees(&ast, &[(ExprId(0), eid)], &generics_with("sv"), &recorded);
        assert!(got.is_empty());
    }

    #[test]
    fn an_unrecorded_generic_call_is_a_hole() {
        let mut ast = Ast::default();
        let eid = call_of(&mut ast, "sv");
        let got = uninferred_callees(
            &ast,
            &[(ExprId(0), eid)],
            &generics_with("sv"),
            &HashMap::new(),
        );
        assert_eq!(got, vec!["sv".to_string()]);
    }

    #[test]
    fn a_call_to_a_non_generic_is_never_a_hole() {
        let mut ast = Ast::default();
        let eid = call_of(&mut ast, "plain");
        let got = uninferred_callees(
            &ast,
            &[(ExprId(0), eid)],
            &generics_with("sv"),
            &HashMap::new(),
        );
        assert!(got.is_empty());
    }

    #[test]
    fn one_callee_reported_once_however_many_sites() {
        let mut ast = Ast::default();
        let a = call_of(&mut ast, "sv");
        let b = call_of(&mut ast, "sv");
        let got = uninferred_callees(
            &ast,
            &[(ExprId(0), a), (ExprId(1), b)],
            &generics_with("sv"),
            &HashMap::new(),
        );
        assert_eq!(got, vec!["sv".to_string()]);
    }
}
