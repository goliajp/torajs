//! An array a program deletes from widens to `any[]` at its
//! declaration.
//!
//! ```text
//! var a = [1, 2, 3];
//! delete a[1];        // bun: true, a[1] undefined, a.length 3
//! ```
//!
//! tr refuses that program outright, and the refusal is correct about
//! the storage it was asked about: deleting is not writing a blank, it
//! has to make the index stop being an own property, and an unboxed
//! `number` slot has no value that means absent
//! ([`crate::check_type_of_misc`] spells this out). An `any`-element
//! array does have one — the hole shadow — and every own-property
//! consumer already agrees with bun on it (`in`, `Object.keys`,
//! `JSON.stringify`, `indexOf` all answer correctly on a hole today).
//!
//! So the declaration is the thing that is wrong, not the delete: a
//! binding the program deletes indices out of was never a `number[]`.
//! Rewriting it to the `: any[]` the user could have written is the
//! move [`super::with_object_widen`] and
//! [`super::objlit_nominal_anylane`] already make for their own
//! dynamic-by-contract bindings, and every existing lane follows —
//! the checker admits the delete, the init lowers through the boxed
//! element lane, and the hole machinery is reached.
//!
//! An explicit annotation is the user's word on the shape and stays.
//! By name, not scope-resolved — the same trade `with_object_widen`
//! documents: a same-named binding elsewhere can widen this one, which
//! costs that binding its unboxed element slots and nothing else.
//!
//! A binding [`crate::let_widen`] already claims — reassigned across
//! syntactic families, `var x = [0]; … x = {…}` — is left alone. That
//! pass types it `any`, which is wider than `any[]` and admits the
//! delete on its own; writing an annotation here would take the
//! binding out of its reach (it tracks UNANNOTATED lets) and turn a
//! program that ran into a refusal. Measured: the
//! `Array.prototype[1] = 1; var x = [0]; … x = {0: 0}` idiom behind
//! nine test262 cases. The claim is read off the prelude AST while
//! `let_widen` decides on the desugared one, so the two can disagree
//! about a shape the class desugar changes; disagreeing costs today's
//! answer and never a new wrong one.
//!
//! Runs in the prelude beside the other `delete` triage, before
//! anything reads element types.

use std::collections::HashSet;

use super::{Ast, Expr, ExprId, Stmt};

/// Names standing as the receiver of a `delete <name>[k]` /
/// `delete <name>.p`, then every unannotated `LetDecl` of such a name
/// whose init is an array literal rewritten to `: any[]`.
pub fn widen_deleted_array_bindings(ast: &mut Ast) {
    let mut names: HashSet<String> = HashSet::new();
    for e in &ast.exprs {
        let Expr::Delete { expr } = e else {
            continue;
        };
        let recv = match &ast.exprs[expr.0 as usize] {
            Expr::Index { obj, .. } | Expr::Member { obj, .. } => *obj,
            _ => continue,
        };
        if let Expr::Ident(n) = &ast.exprs[recv.0 as usize] {
            names.insert(n.clone());
        }
    }
    if names.is_empty() {
        return;
    }
    let claimed = crate::let_widen::collect_cross_type_widen_inits(ast);
    let mut stmts = std::mem::take(&mut ast.stmts);
    // The mutable spine is FULLY recursive (each nested list handed to
    // the callback exactly once), so the per-list body must not
    // recurse itself.
    widen_flat(&mut stmts, &ast.exprs, &names, &claimed);
    for s in stmts.iter_mut() {
        super::stmt_nested_lists::for_each_nested_vec_mut(s, &mut |inner| {
            widen_flat(inner, &ast.exprs, &names, &claimed)
        });
    }
    ast.stmts = stmts;
}

fn widen_flat(
    stmts: &mut [Stmt],
    exprs: &[Expr],
    names: &HashSet<String>,
    claimed: &HashSet<ExprId>,
) {
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
        if type_ann.is_some() || !names.contains(name) || claimed.contains(init) {
            continue;
        }
        if matches!(peel_as(exprs, *init), Expr::Array(_)) {
            *type_ann = Some("any[]".to_string());
        }
    }
}

/// TS casts are static assertions — look at the value underneath.
fn peel_as<'e>(exprs: &'e [Expr], mut e: ExprId) -> &'e Expr {
    loop {
        match &exprs[e.0 as usize] {
            Expr::As { expr, .. } => e = *expr,
            other => return other,
        }
    }
}
