//! Annotation-predicate binding walks (chunk 765 extraction from
//! [`crate::ast_collect_fn_closure`], which had drifted past the
//! 500-line file limit as store-site axes stacked up).
//!
//! Collect `let`/`const`/`var` binding NAMES whose annotation
//! matches a predicate, recursing through fn bodies and every
//! statement container. Three consumers, differing only in the
//! annotation test:
//!
//! - [`collect_any_bindings`] — `any`-annotated bindings (the
//!   assign-into-any / call-arg-into-any wrap axes).
//! - [`collect_fn_arr_bindings`] — fn-typed ARRAY annotations
//!   (chunk 733; element slots are Closure-repr).
//! - [`collect_fn_ann_bindings`] — plain fn-type annotations at any
//!   scope depth (chunk 736; variadic anns keep their boxed-dual
//!   route).

use std::collections::{HashMap, HashSet};

use crate::ast::{Stmt, is_fn_like_ann};

/// Collect every `let`/`const`/`var` binding name carrying an `any`
/// annotation, recursing through fn bodies and statement containers.
pub(crate) fn collect_any_bindings(stmts: &[Stmt], out: &mut HashSet<String>) {
    // A `var` binding counts whatever the source says, on the same
    // grounds V4 knife 3 gave the init axis: the hoist pass — which
    // runs after this walk — mints every hoisted binding as `any` on
    // purpose, since a pre-init read is `undefined` and a var may be
    // reassigned across types. Reading only the written annotation
    // left `var x; x = foo;` panicking at box_to_any while
    // `var x = foo` (knife 3) and `let x: any; x = foo` both worked.
    walk_bindings(stmts, &mut |name, ann, _, is_var| {
        if is_var || ann.is_some_and(|a| a.trim() == "any") {
            out.insert(name.to_string());
        }
    });
}

/// Chunk 733 — binding names declared with a fn-typed array
/// annotation (`((n)=>n)[]` / `Array<(n)=>n>`); their element slots
/// are Closure-repr, so named-fn store-sites into them wrap.
pub(crate) fn collect_fn_arr_bindings(stmts: &[Stmt], out: &mut HashSet<String>) {
    collect_bindings_matching(stmts, &crate::ast::is_fn_arr_ann, out);
}

/// Chunk 736 — binding names declared with a plain fn-type
/// annotation at ANY scope depth (the closure_bindings top-level
/// walk misses fn-body `let cb: (n)=>n` bindings, so a body-local
/// `cb = top_fn` assign escaped the wrap). Variadic anns keep their
/// boxed-dual route.
pub(crate) fn collect_fn_ann_bindings(stmts: &[Stmt], out: &mut HashSet<String>) {
    collect_bindings_matching(stmts, &|a| is_fn_like_ann(a) && !a.contains("__rest("), out);
}

/// RFC 20260729-fn-value-any V1b — UN-annotated binding names whose
/// init is a constructor call (`let f = new C()`; by wrap time the
/// class/fn-ctor desugars have rewritten `new` into a `__new_*`
/// factory call). Such a receiver is a class instance / dynobj the
/// checker types Any-ward — its method calls ride the runtime
/// any-method lane, so a fn-name argument must wrap (the V1 axis's
/// "typed ident receiver" exemption must NOT cover it: `let f = new
/// foo(); f.every(cb)` was the box_to_any FnSig residue's largest
/// shape).
pub(crate) fn collect_untyped_new_init_bindings(
    ast: &crate::ast::Ast,
    stmts: &[Stmt],
    out: &mut HashSet<String>,
) {
    walk_bindings(stmts, &mut |name, ann, init, _| {
        let ctor_init = match ast.get_expr(init) {
            crate::ast::Expr::New { .. } => true,
            crate::ast::Expr::Call { callee, .. } => matches!(
                ast.get_expr(*callee),
                // `__new_` = class factory; `__fnctor_` = the
                // repointed fn-constructor factory (`new foo()` where
                // foo is a plain function).
                crate::ast::Expr::Ident(n)
                    if n.starts_with("__new_") || n.starts_with("__fnctor_")
            ),
            _ => false,
        };
        if ann.is_none() && ctor_init {
            out.insert(name.to_string());
        }
    });
}

/// Chunk 783 — binding name → declared type-ann (trimmed) for every
/// binding whose annotation satisfies `pred`. The member-assign wrap
/// axis (`o.cb = top_fn`) resolves the receiver binding to its
/// declared struct shape through this map. Scope-approximate like
/// the sets above: a shadowing binding of another matching type
/// overwrites (last one wins), which at worst costs an unnecessary
/// wrap.
pub(crate) fn collect_bindings_ann_matching(
    stmts: &[Stmt],
    pred: &dyn Fn(&str) -> bool,
    out: &mut HashMap<String, String>,
) {
    walk_bindings(stmts, &mut |name, ann, _, _| {
        if let Some(ann) = ann
            && pred(ann)
        {
            out.insert(name.to_string(), ann.trim().to_string());
        }
    });
}

/// Shared annotation-predicate binding walk (chunk 733 — the any
/// and fn-arr collections differ only in the ann test).
fn collect_bindings_matching(
    stmts: &[Stmt],
    pred: &dyn Fn(&str) -> bool,
    out: &mut HashSet<String>,
) {
    walk_bindings(stmts, &mut |name, ann, _, _| {
        if let Some(ann) = ann
            && pred(ann)
        {
            out.insert(name.to_string());
        }
    });
}

/// Visit every annotated `let`/`const`/`var` binding (name, ann)
/// recursing through fn bodies and statement containers — the shared
/// walker behind the set and map collections (chunk 783 rework).
fn walk_bindings(stmts: &[Stmt], f: &mut dyn FnMut(&str, Option<&str>, crate::ast::ExprId, bool)) {
    for s in stmts {
        walk_bindings_stmt(s, f);
    }
}

fn walk_bindings_stmt(s: &Stmt, f: &mut dyn FnMut(&str, Option<&str>, crate::ast::ExprId, bool)) {
    match s {
        Stmt::LetDecl {
            name,
            type_ann,
            init,
            is_var,
            ..
        } => {
            f(name, type_ann.as_deref(), *init, *is_var);
        }
        Stmt::FnDecl { body, .. } => walk_bindings(body, f),
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            walk_bindings_stmt(then_branch, f);
            if let Some(eb) = else_branch {
                walk_bindings_stmt(eb, f);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => walk_bindings_stmt(body, f),
        Stmt::Labeled { body, .. } => walk_bindings_stmt(body, f),
        Stmt::For { init, body, .. } => {
            if let Some(init) = init {
                walk_bindings_stmt(init, f);
            }
            walk_bindings_stmt(body, f);
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => walk_bindings(stmts, f),
        Stmt::Switch { cases, default, .. } => {
            for c in cases {
                walk_bindings(&c.body, f);
            }
            if let Some(d) = default {
                walk_bindings(d, f);
            }
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            walk_bindings(body, f);
            walk_bindings(catch_body, f);
            if let Some(fb) = finally_body {
                walk_bindings(fb, f);
            }
        }
        _ => {}
    }
}

/// Collect every name the fn body binds locally (params handled by
/// the caller): let/const decls, for-of loop vars, catch params —
/// recursing through nested blocks AND nested inline FnDecls (their
/// locals over-shadow the outer walk, which only ever skips a wrap;
/// the nested FnDecl arm pushes its own precise frame anyway).
pub(crate) fn collect_local_binding_names(body: &[Stmt], out: &mut HashSet<String>) {
    for s in body {
        match s {
            Stmt::LetDecl { name, .. } => {
                out.insert(name.clone());
            }
            Stmt::FnDecl { params, body, .. } => {
                for p in params {
                    out.insert(p.name.clone());
                }
                collect_local_binding_names(body, out);
            }
            Stmt::ForOf {
                var_name,
                i_ident,
                body,
                ..
            } => {
                out.insert(var_name.clone());
                out.insert(i_ident.clone());
                collect_local_binding_names(core::slice::from_ref(body), out);
            }
            Stmt::Try {
                body,
                catch_param,
                catch_body,
                finally_body,
                ..
            } => {
                if let Some(cp) = catch_param {
                    out.insert(cp.clone());
                }
                collect_local_binding_names(body, out);
                collect_local_binding_names(catch_body, out);
                if let Some(fb) = finally_body {
                    collect_local_binding_names(fb, out);
                }
            }
            // `let a, b;` parses into a Multi of per-name LetDecls —
            // without this arm both names were invisible to every
            // consumer (delete triage / readonly shadows / implicit
            // globals), and a synthesized `var` collided with the
            // real `let` (rotation 444 sweep regression).
            Stmt::Block(inner) | Stmt::Multi(inner) => collect_local_binding_names(inner, out),
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_local_binding_names(core::slice::from_ref(then_branch), out);
                if let Some(eb) = else_branch {
                    collect_local_binding_names(core::slice::from_ref(eb), out);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                collect_local_binding_names(core::slice::from_ref(body), out);
            }
            Stmt::Labeled { body, .. } => {
                collect_local_binding_names(core::slice::from_ref(body), out);
            }
            Stmt::For { init, body, .. } => {
                if let Some(i) = init {
                    collect_local_binding_names(core::slice::from_ref(i), out);
                }
                collect_local_binding_names(core::slice::from_ref(body), out);
            }
            Stmt::Switch { cases, default, .. } => {
                for c in cases {
                    collect_local_binding_names(&c.body, out);
                }
                if let Some(d) = default {
                    collect_local_binding_names(d, out);
                }
            }
            _ => {}
        }
    }
}
