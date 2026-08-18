//! Receiver-binding collectors for [`super::fnexpr_this`] (knife 4's
//! syntactically-certain HOF receivers) — split out so the parent
//! stays under the 500-prod-LOC file-size hard limit. Bodies
//! verbatim.
//!
//! Every census here recurses through the SHARED nested-list spine
//! ([`super::stmt_nested_lists::for_each_nested_list`]) — rotation
//! 437. The hand-rolled FnDecl/Block-only recursion these walks
//! carried before skipped every other compound form, exactly the
//! failure `stmt_nested_lists`' doc warns about: a `with` statement
//! inside a `try` desugars to a Block holding the `__with_<n>`
//! binding, the any-census never saw that declaration, and the
//! store-face promote on `__with_<n>.f = function () { …this… }`
//! silently did not fire — the same program promoted at top level
//! and refused inside `try`. The by-name conflict sets widen on the
//! same spine, so the over-removal posture is unchanged.

use super::{Expr, Stmt};

/// Binding names that are `: any` at EVERY declaration in the
/// program (top-level walk; a same-name non-any declaration anywhere
/// removes the name — over-removal only keeps a face loud, never
/// mis-promotes a typed receiver).
///
/// An UNANNOTATED empty-object-literal init (`var obj = {}`) joins
/// the set: `{}` has no struct surface, so the binding is a dynobj
/// and every expando-stored method call rides the runtime any-method
/// dispatch — the same receiver-seeding lane the `: any` spelling
/// uses (test262's dominant custom-matcher receiver shape).
pub(super) fn collect_any_binding_names(
    stmts: &[Stmt],
    exprs: &[Expr],
) -> std::collections::HashSet<String> {
    let mut any_names = std::collections::HashSet::new();
    let mut other_names = std::collections::HashSet::new();
    collect_binding_names_inner(stmts, exprs, &mut any_names, &mut other_names);
    any_names.retain(|n| !other_names.contains(n));
    any_names
}

fn collect_binding_names_inner(
    stmts: &[Stmt],
    exprs: &[Expr],
    any_names: &mut std::collections::HashSet<String>,
    other_names: &mut std::collections::HashSet<String>,
) {
    for s in stmts {
        match s {
            Stmt::LetDecl {
                name,
                type_ann,
                init,
                ..
            } => {
                let dynobj_certain = type_ann.is_none()
                    && matches!(&exprs[init.0 as usize], Expr::ObjectLit { fields } if fields.is_empty());
                if type_ann.as_deref() == Some("any") || dynobj_certain {
                    any_names.insert(name.clone());
                } else {
                    other_names.insert(name.clone());
                }
            }
            _ => {}
        }
        super::stmt_nested_lists::for_each_nested_list(s, &mut |inner| {
            collect_binding_names_inner(inner, exprs, any_names, other_names)
        });
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
            _ => {}
        }
        super::stmt_nested_lists::for_each_nested_list(s, &mut |inner| {
            collect_arraylit_names_inner(inner, exprs, names, other)
        });
    }
}

/// Array-literal inits of `: any` bindings — the 399-02 face
/// position. An element-position fn-expr promotes because the
/// annotation makes every element read a box: `arr[0]` rides the any
/// lane end to end (`.call` / `.apply` seed the given receiver; an
/// index-call seeds the array itself — §13.3.6.2, the 399-05 leg
/// closed before this arm landed), and every one of those paths
/// shifts argv on FLAG_CLOSURE_RECV_FIRST. Gated on
/// [`collect_any_binding_names`]'s every-declaration-is-any set —
/// same walk spine, so the gate sees every declaration this walk can
/// find and a same-name typed declaration anywhere keeps the face
/// loud (over-removal posture).
pub(super) fn collect_any_arraylit_inits(
    stmts: &[Stmt],
    exprs: &[Expr],
    any_recvs: &std::collections::HashSet<String>,
    out: &mut std::collections::HashSet<super::ExprId>,
) {
    for s in stmts {
        match s {
            Stmt::LetDecl {
                name,
                type_ann,
                init,
                ..
            } => {
                if type_ann.as_deref() == Some("any")
                    && any_recvs.contains(name)
                    && matches!(&exprs[init.0 as usize], Expr::Array(_))
                {
                    out.insert(*init);
                }
            }
            _ => {}
        }
        super::stmt_nested_lists::for_each_nested_list(s, &mut |inner| {
            collect_any_arraylit_inits(inner, exprs, any_recvs, out)
        });
    }
}

/// Immutable bindings that are syntactically-certain Map / Set
/// receivers: the init is `new Map(...)` / `new Set(...)` (any
/// instantiation spelling), or the declaration carries an explicit
/// Map / Set annotation. Same over-removal posture as the collectors
/// above — a same-name re-declaration anywhere keeps the face loud.
pub(super) fn collect_mapset_binding_names(
    stmts: &[Stmt],
    exprs: &[Expr],
) -> std::collections::HashSet<String> {
    let mut names = std::collections::HashSet::new();
    let mut other = std::collections::HashSet::new();
    collect_mapset_names_inner(stmts, exprs, &mut names, &mut other);
    names.retain(|n| !other.contains(n));
    names
}

fn is_mapset_ann(ann: &str) -> bool {
    matches!(ann, "Map" | "Set")
        || ann.strip_prefix("Map<").is_some_and(|r| r.ends_with('>'))
        || ann.strip_prefix("Set<").is_some_and(|r| r.ends_with('>'))
}

fn collect_mapset_names_inner(
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
                type_ann,
                init,
                ..
            } if match type_ann {
                Some(t) => is_mapset_ann(t),
                None => matches!(&exprs[init.0 as usize],
                    Expr::New { class_name, .. } if class_name == "Map" || class_name == "Set"),
            } =>
            {
                names.insert(name.clone());
            }
            Stmt::LetDecl { name, .. } => {
                other.insert(name.clone());
            }
            _ => {}
        }
        super::stmt_nested_lists::for_each_nested_list(s, &mut |inner| {
            collect_mapset_names_inner(inner, exprs, names, other)
        });
    }
}

/// Bindings that are syntactically-certain generator-object
/// receivers: the init is a direct call of a `function*` declared in
/// the same program (`let iter = g()`). Same over-removal posture as
/// the collectors above — a same-name re-declaration anywhere keeps
/// the face loud. The iterator-helper kernels (eager and lazy alike)
/// read `FLAG_CLOSURE_RECV_FIRST` off the callback and seed its
/// receiver slot `undefined`, which is §27.1.4's Call(cb, undefined)
/// exactly.
pub(super) fn collect_gen_iter_binding_names(
    stmts: &[Stmt],
    exprs: &[Expr],
) -> std::collections::HashSet<String> {
    let mut gen_fns = std::collections::HashSet::new();
    collect_gen_fn_names(stmts, &mut gen_fns);
    let mut names = std::collections::HashSet::new();
    let mut other = std::collections::HashSet::new();
    collect_gen_iter_names_inner(stmts, exprs, &gen_fns, &mut names, &mut other);
    names.retain(|n| !other.contains(n));
    names
}

fn collect_gen_fn_names(stmts: &[Stmt], out: &mut std::collections::HashSet<String>) {
    for s in stmts {
        match s {
            // `desugar_generators` has already run: a `function* g`
            // is now a factory FnDecl still named `g` whose declared
            // return type is the synthesized `__Gen_<name>` class.
            // (`is_generator` is false again by this point, so the
            // return-type spelling is the surviving marker.)
            Stmt::FnDecl {
                name,
                return_type: Some(rt),
                ..
            } => {
                if rt.starts_with("__Gen_") {
                    out.insert(name.clone());
                }
            }
            _ => {}
        }
        super::stmt_nested_lists::for_each_nested_list(s, &mut |inner| {
            collect_gen_fn_names(inner, out)
        });
    }
}

fn collect_gen_iter_names_inner(
    stmts: &[Stmt],
    exprs: &[Expr],
    gen_fns: &std::collections::HashSet<String>,
    names: &mut std::collections::HashSet<String>,
    other: &mut std::collections::HashSet<String>,
) {
    for s in stmts {
        match s {
            Stmt::LetDecl { name, init, .. }
                if matches!(&exprs[init.0 as usize], Expr::Call { callee, args }
                    if args.is_empty()
                        && matches!(&exprs[callee.0 as usize], Expr::Ident(f) if gen_fns.contains(f))) =>
            {
                names.insert(name.clone());
            }
            Stmt::LetDecl { name, .. } => {
                other.insert(name.clone());
            }
            _ => {}
        }
        super::stmt_nested_lists::for_each_nested_list(s, &mut |inner| {
            collect_gen_iter_names_inner(inner, exprs, gen_fns, names, other)
        });
    }
}
