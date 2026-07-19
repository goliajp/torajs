//! Receiver-binding collectors for [`super::fnexpr_this`] (knife 4's
//! syntactically-certain HOF receivers) — split out so the parent
//! stays under the 500-prod-LOC file-size hard limit. Bodies
//! verbatim.

use super::{Expr, Stmt};

/// Binding names that are `: any` at EVERY declaration in the
/// program (top-level walk; a same-name non-any declaration anywhere
/// removes the name — over-removal only keeps a face loud, never
/// mis-promotes a typed receiver).
pub(super) fn collect_any_binding_names(stmts: &[Stmt]) -> std::collections::HashSet<String> {
    let mut any_names = std::collections::HashSet::new();
    let mut other_names = std::collections::HashSet::new();
    collect_binding_names_inner(stmts, &mut any_names, &mut other_names);
    any_names.retain(|n| !other_names.contains(n));
    any_names
}

fn collect_binding_names_inner(
    stmts: &[Stmt],
    any_names: &mut std::collections::HashSet<String>,
    other_names: &mut std::collections::HashSet<String>,
) {
    for s in stmts {
        match s {
            Stmt::LetDecl { name, type_ann, .. } => {
                if type_ann.as_deref() == Some("any") {
                    any_names.insert(name.clone());
                } else {
                    other_names.insert(name.clone());
                }
            }
            Stmt::FnDecl { body, .. } => {
                collect_binding_names_inner(body, any_names, other_names);
            }
            Stmt::Block(inner) | Stmt::Multi(inner) => {
                collect_binding_names_inner(inner, any_names, other_names);
            }
            _ => {}
        }
    }
}

/// Immutable bindings whose init is a literal array (and whose name
/// is never re-declared) — the syntactically-certain typed-Arr
/// receivers for the knife-4 typed HOF trio. Same over-removal
/// posture as [`collect_any_binding_names`].
pub(super) fn collect_arraylit_binding_names(
    stmts: &[Stmt],
    exprs: &[Expr],
) -> std::collections::HashSet<String> {
    let mut names = std::collections::HashSet::new();
    let mut other = std::collections::HashSet::new();
    collect_arraylit_names_inner(stmts, exprs, &mut names, &mut other);
    names.retain(|n| !other.contains(n));
    names
}

fn collect_arraylit_names_inner(
    stmts: &[Stmt],
    exprs: &[Expr],
    names: &mut std::collections::HashSet<String>,
    other: &mut std::collections::HashSet<String>,
) {
    for s in stmts {
        match s {
            Stmt::LetDecl {
                mutable: false,
                name,
                type_ann: None,
                init,
                ..
            } if matches!(&exprs[init.0 as usize], Expr::Array(_)) => {
                names.insert(name.clone());
            }
            Stmt::LetDecl { name, .. } => {
                other.insert(name.clone());
            }
            Stmt::FnDecl { body, .. } => {
                collect_arraylit_names_inner(body, exprs, names, other);
            }
            Stmt::Block(inner) | Stmt::Multi(inner) => {
                collect_arraylit_names_inner(inner, exprs, names, other);
            }
            _ => {}
        }
    }
}

/// Does the lifted FnDecl named `fn_name` declare a rest param
/// (`...args`)? Rest-tail closures dispatch through the boxed
/// variadic entry, which materializes params off argv — cut 2's
/// mixed promotion excludes them.
pub(super) fn fn_has_rest_param(stmts: &[Stmt], fn_name: &str) -> bool {
    stmts.iter().any(|s| {
        matches!(s, Stmt::FnDecl { name, params, .. }
            if name == fn_name && params.iter().any(|p| p.is_rest))
    })
}
