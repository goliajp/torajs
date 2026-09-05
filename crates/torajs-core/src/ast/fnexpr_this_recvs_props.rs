//! The runtime-props receiver census — the B2 store-receiver bar.
//!
//! Moved out of [`super::fnexpr_this_recvs`] verbatim in rotation 594:
//! that file stood at 478 prod lines, and the next thing this census
//! needs is an arm for a CALL initializer, which does not fit under the
//! 500-line limit alongside the five other censuses. Nothing else
//! changed in the move.

use super::{Expr, Stmt};

/// Runtime-props receiver bindings for the B2 store-receiver bar
/// (RFC 20260808-construct-channel, species key 2) — a name admits
/// when its EVERY declaration is one of the runtime-props shapes:
/// an `: any` / `T[]`-annotated binding, an unannotated `{}` init,
/// or an unannotated array-literal init. One walk over BOTH shape
/// families replaces the retired lenient array collector: test262
/// harness helpers commonly declare `const a: any` while the case
/// body declares `var a = []` under the same name, and two
/// separately-walked collectors each classified the other's shape
/// as "other" — the over-removal killed the name in both sets, so
/// the store arm never admitted the case's
/// `a.constructor[Symbol.species]` store. The store-receiver
/// argument only needs "this root's stored face is a runtime props
/// slot, never a typed struct field", which every one of these
/// shapes preserves across a `var` reassignment (the checker's type
/// discipline keeps an array-typed binding an array). The knife-4
/// HOF collectors keep their stricter bars — their consumers pair a
/// receiver VALUE with a callback, where a rebind changes which
/// cell the promoted callback meets.
pub(super) fn collect_props_receiver_binding_names(
    stmts: &[Stmt],
    exprs: &[Expr],
) -> std::collections::HashSet<String> {
    let mut names = std::collections::HashSet::new();
    let mut other = std::collections::HashSet::new();
    collect_props_receiver_inner(stmts, exprs, &mut names, &mut other);
    names.retain(|n| !other.contains(n));
    names
}

fn collect_props_receiver_inner(
    stmts: &[Stmt],
    exprs: &[Expr],
    names: &mut std::collections::HashSet<String>,
    other: &mut std::collections::HashSet<String>,
) {
    for s in stmts {
        match s {
            Stmt::LetDecl {
                name,
                type_ann,
                init,
                ..
            } => {
                let props_shape = match type_ann.as_deref() {
                    Some(a) => a == "any" || a.ends_with("[]"),
                    None => match &exprs[init.0 as usize] {
                        Expr::Array(_) => true,
                        Expr::ObjectLit { fields } => fields.is_empty(),
                        _ => false,
                    },
                };
                if props_shape {
                    names.insert(name.clone());
                } else {
                    other.insert(name.clone());
                }
            }
            _ => {}
        }
        super::stmt_nested_lists::for_each_nested_list(s, &mut |inner| {
            collect_props_receiver_inner(inner, exprs, names, other)
        });
    }
}
