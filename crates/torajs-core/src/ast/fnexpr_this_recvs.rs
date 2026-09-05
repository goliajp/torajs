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

/// Bindings whose cell is certainly the literal array the program
/// wrote — the syntactically-certain typed-Arr receivers for the
/// knife-4 typed HOF trio. Same over-removal posture as
/// [`collect_any_binding_names`].
///
/// Two proofs admit a name, and they prove the same thing (the
/// `fnexpr_this_default` census carries the argument in full). A
/// `const … = […]` cannot be re-pointed. A MUTABLE `var arr = […]`
/// is just as settled when the whole program never writes the name
/// again after that initializer — test262 spells its receivers
/// `var arr = [1]` far more often than `const`, and the knife-4
/// consumers run BEFORE `desugar_var_hoist`, so the declaration
/// still carries its real initializer here. The write census is a
/// flat arena scan (every nested body's exprs live in the one
/// arena; detached nodes can only over-refuse, which costs a loud
/// reject, never a mis-promote).
pub(super) fn collect_arraylit_binding_names(
    stmts: &[Stmt],
    exprs: &[Expr],
) -> std::collections::HashSet<String> {
    let mut names = std::collections::HashSet::new();
    let mut mut_once = std::collections::HashSet::new();
    let mut other = std::collections::HashSet::new();
    collect_arraylit_names_inner(stmts, exprs, &mut names, &mut mut_once, &mut other);
    // `Assign` and `PostIncr` are the only two expression forms that
    // write a bare name (pre-increment desugars to Assign).
    for e in exprs {
        if let Expr::Assign { target, .. } | Expr::PostIncr { target, .. } = e
            && let Expr::Ident(n) = &exprs[target.0 as usize]
        {
            mut_once.remove(n);
        }
    }
    names.extend(mut_once);
    names.retain(|n| !other.contains(n));
    names
}

fn collect_arraylit_names_inner(
    stmts: &[Stmt],
    exprs: &[Expr],
    names: &mut std::collections::HashSet<String>,
    mut_once: &mut std::collections::HashSet<String>,
    other: &mut std::collections::HashSet<String>,
) {
    for s in stmts {
        match s {
            Stmt::LetDecl {
                mutable,
                name,
                type_ann: None,
                init,
                ..
            } if matches!(&exprs[init.0 as usize], Expr::Array(_)) => {
                // A second declaration of the name — whatever its
                // shape — drops it: certainty is one binding, once.
                if names.contains(name) || mut_once.contains(name) {
                    other.insert(name.clone());
                } else if *mutable {
                    mut_once.insert(name.clone());
                } else {
                    names.insert(name.clone());
                }
            }
            Stmt::LetDecl { name, .. } => {
                other.insert(name.clone());
            }
            _ => {}
        }
        super::stmt_nested_lists::for_each_nested_list(s, &mut |inner| {
            collect_arraylit_names_inner(inner, exprs, names, mut_once, other)
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

/// Bindings that are syntactically-certain String-wrapper receivers
/// — every reaching value is a `new String(...)` mint, so a method
/// call on the name rides the runtime any-dispatch's StringWrapper
/// view-through into `str_method`, whose replace-family functional
/// kernels all read `FLAG_CLOSURE_RECV_FIRST` (rotation 447; the
/// inline-New admission lives in
/// `fnexpr_this_cb_slots::collect_replace_face`).
///
/// Two decl spellings reach here because the census runs AFTER
/// `desugar_var_hoist`: a kept decl (annotated `var`, or
/// `let`/`const`) still carries its `new String` init, while a
/// hoisted `var` splits into the prelude's `: any` + `Uninit` decl
/// plus exactly one `Expr::Assign` whose value is the mint — that
/// neutral decl shape neither admits nor blocks, the assign walk
/// decides.
///
/// Over-removal posture: a same-name decl with any other init, ANY
/// assignment to a decl-admitted name, more than one mint
/// assignment, a non-mint assignment, or a non-LetDecl shadow
/// anywhere keeps the face loud — a rebind could hand the promoted
/// callback to a user-defined `replace` on a path with no
/// receiver-flag reader.
pub(super) fn collect_strwrapper_binding_names(
    stmts: &[Stmt],
    exprs: &[Expr],
) -> std::collections::HashSet<String> {
    let mut decl_mint = std::collections::HashSet::new();
    let mut neutral = std::collections::HashSet::new();
    let mut other = std::collections::HashSet::new();
    collect_strwrapper_names_inner(stmts, exprs, &mut decl_mint, &mut neutral, &mut other);
    // Assign spine — one arena walk counts, per target name, the
    // mint assignments and every other assignment.
    let mut mints: std::collections::HashMap<&str, u32> = std::collections::HashMap::new();
    let mut others: std::collections::HashMap<&str, u32> = std::collections::HashMap::new();
    for e in exprs {
        let Expr::Assign { target, value } = e else {
            continue;
        };
        let Expr::Ident(n) = &exprs[target.0 as usize] else {
            continue;
        };
        let v = super::fnexpr_this_names::peel_as(exprs, *value);
        if is_string_mint(exprs, v) {
            *mints.entry(n.as_str()).or_default() += 1;
        } else {
            *others.entry(n.as_str()).or_default() += 1;
        }
    }
    let mut names: std::collections::HashSet<String> = decl_mint
        .into_iter()
        .filter(|n| !mints.contains_key(n.as_str()) && !others.contains_key(n.as_str()))
        .collect();
    names.extend(
        neutral
            .into_iter()
            .filter(|n| mints.get(n.as_str()) == Some(&1) && !others.contains_key(n.as_str())),
    );
    names.retain(|n| {
        !other.contains(n) && !super::fnexpr_this_names::name_shadowed_elsewhere(stmts, n)
    });
    names
}

fn is_string_mint(exprs: &[Expr], e: super::ExprId) -> bool {
    matches!(&exprs[e.0 as usize], Expr::New { class_name, .. } if class_name == "String")
}

fn collect_strwrapper_names_inner(
    stmts: &[Stmt],
    exprs: &[Expr],
    decl_mint: &mut std::collections::HashSet<String>,
    neutral: &mut std::collections::HashSet<String>,
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
                if is_string_mint(exprs, super::fnexpr_this_names::peel_as(exprs, *init)) {
                    decl_mint.insert(name.clone());
                } else if type_ann.as_deref() == Some("any")
                    && matches!(&exprs[init.0 as usize], Expr::Uninit)
                {
                    neutral.insert(name.clone());
                } else {
                    other.insert(name.clone());
                }
            }
            _ => {}
        }
        super::stmt_nested_lists::for_each_nested_list(s, &mut |inner| {
            collect_strwrapper_names_inner(inner, exprs, decl_mint, neutral, other)
        });
    }
}
