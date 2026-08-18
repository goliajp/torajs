//! §14.11 — an object literal used as a `with` scope object widens to
//! `any` at its declaration (rotation 437).
//!
//! The object at the front of a `with` chain is DYNAMIC by contract:
//! §9.1.1.2.1 HasBinding is re-evaluated per reference, a body-side
//! `delete` removes real properties (`delete p3` deletes `o.p3`), a
//! getter can delete its own backing slot mid-read (the S11.13.2
//! compound-assignment family), and every bare-name miss must answer
//! through the fall-through rather than a struct layout. A nominal
//! struct stamp can express none of that — the checker refused
//! `delete` on the typed layout and rejected every member the literal
//! did not spell, which is exactly §14.11's fall-through case.
//!
//! The move is the detached-objlit-method one
//! ([`super::objlit_nominal_anylane::widen_detached_method_objlits`]):
//! rewrite the DECLARATION to the `: any` the user could have
//! written, and every existing lane follows — the SSA `let x: any =
//! {…}` shortcut lowers the literal through the dynobj lane, the
//! checker types every downstream read `any`, and `objlit_nominal`'s
//! (a) leg picks the binding up when the literal carries methods. An
//! explicit annotation is the user's word on the shape and stays; a
//! LITERAL `with` head (`with ({…})`) needs nothing here — it flows
//! into `__torajs_with_obj`'s `: any` parameter, which is the
//! anylane (f) leg.
//!
//! Runs after `desugar_with` (the head expression survives as the
//! helper call's argument) and before the objlit passes. By-name,
//! not scope-resolved — the same trade the detached-method widen
//! documents: a same-named binding elsewhere can widen this one,
//! which only ever costs that binding's struct-offset reads.

use super::{Ast, Expr, Stmt};

/// Names standing as `__torajs_with_obj(name)` arguments, then every
/// unannotated `LetDecl` of such a name whose init is an object
/// literal rewritten to `: any`.
///
/// A NESTED `with` head is itself a free name of the enclosing body,
/// so the desugar has already wrapped it in the reference guard —
/// `__torajs_with_obj((has(w, "inner") ? w.inner : inner))` — and the
/// user binding sits on the ternary's fall-through leg. Peeling to
/// that leg is sound: the leg IS the binding read the guard preserves
/// (`with_fallthrough_idents`), and the then-leg's runtime object is
/// the enclosing scope object, which this same walk widens through
/// its own head.
pub fn widen_with_object_bindings(ast: &mut Ast) {
    let mut names: std::collections::HashSet<String> = std::collections::HashSet::new();
    for e in &ast.exprs {
        if let Expr::Call { callee, args } = e
            && matches!(&ast.exprs[callee.0 as usize], Expr::Ident(f) if f == super::desugar_with::WITH_OBJ_FN)
            && let Some(a) = args.first()
        {
            let mut a = *a;
            while let Expr::Ternary {
                cond, else_branch, ..
            } = &ast.exprs[a.0 as usize]
            {
                let guard = matches!(&ast.exprs[cond.0 as usize], Expr::Call { callee, .. }
                    if matches!(&ast.exprs[callee.0 as usize],
                        Expr::Ident(f) if f == super::desugar_with::WITH_HAS_FN));
                if !guard {
                    break;
                }
                a = *else_branch;
            }
            if let Expr::Ident(n) = &ast.exprs[a.0 as usize] {
                names.insert(n.clone());
            }
        }
    }
    if names.is_empty() {
        return;
    }
    let mut stmts = std::mem::take(&mut ast.stmts);
    // The mutable spine is FULLY recursive (each nested list handed
    // to the callback exactly once), so the per-list body must not
    // recurse itself.
    widen_flat(&mut stmts, &ast.exprs, &names);
    for s in stmts.iter_mut() {
        super::stmt_nested_lists::for_each_nested_vec_mut(s, &mut |inner| {
            widen_flat(inner, &ast.exprs, &names)
        });
    }
    ast.stmts = stmts;
}

fn widen_flat(stmts: &mut [Stmt], exprs: &[Expr], names: &std::collections::HashSet<String>) {
    for s in stmts.iter_mut() {
        let Stmt::LetDecl {
            name,
            type_ann,
            init,
            ..
        } = s
        else {
            continue;
        };
        if type_ann.is_some() || !names.contains(name) {
            continue;
        }
        let mut cur = *init;
        while let Expr::As { expr, .. } = &exprs[cur.0 as usize] {
            cur = *expr;
        }
        // A computed-key literal is already served whole by the
        // anylane (g) leg (the checker types it `any` at every
        // position), so the widen adds nothing there — and it MOVED
        // something: the `[Symbol.unscopables]` env shape's deleted-
        // binding read lost its strict-mode ReferenceError (measured,
        // rotation 437 sweep regression). The honest §9.1.1.2.6
        // GetBindingValue strict-reference recheck is an L3b item;
        // until it exists these literals keep their pre-widen lane.
        if matches!(&exprs[cur.0 as usize], Expr::ObjectLit { fields }
            if !fields.iter().any(|(n, _)| n.starts_with("__computed_")))
        {
            *type_ann = Some("any".to_string());
        }
    }
}
