//! 424-04 — mismatch census for fn-typed bindings (mutable ones,
//! and immutable ones initialized from a function literal).
//!
//! A mutable fn-typed binding is a closure_bindings member, so every
//! store site (init and assign rhs) wraps into a `__forward_*`
//! closure cell — but the bare closure-call lane still fires a
//! `call_indirect` shaped by the binding's ANNOTATION. When a stored
//! fn declares MORE params than the annotation spells (the S2
//! excess-Any-tail admit: `let slot: () => void = gb` where
//! `gb(p = mk())` materialized to `gb(p: any)`), the callee reads an
//! argument register the caller never filled — silent garbage. The
//! immutable flavor reabstracts through a sig-thunk
//! ([`super::sig_thunk`]); a MUTABLE binding can hold different
//! faces over its lifetime, so no single thunk fits. Instead the
//! binding's calls ride the boxed dual entry (argc +
//! undefined-filled argv + the per-face `__boxed_*` adapter), the
//! same lane a rest-typed binding rides — the adapter unboxes per
//! the CALLEE's declared face, so §10.2.1.4 undefined-binding and
//! the materialized default guards hold for every stored face.
//!
//! This census only marks PROVEN mismatches (a store whose rhs is a
//! known top-level FnDecl Ident with a non-sig-exact face) — a
//! sig-exact binding keeps the bare indirect call, which is the
//! whole point of the FnSig fast lane. Generator / rest targets are
//! skipped: rest rides its own variadic route and a generator
//! factory's calling face is owned by the generator lanes.

use super::{Ast, Expr, Param, Stmt};
use std::collections::{HashMap, HashSet};

/// A param annotation as a comparable spelling: absent/empty means
/// `any` (the checker's default for an unannotated param).
fn norm_ann(ann: Option<&str>) -> &str {
    match ann {
        Some(a) => {
            let t = a.trim();
            if t.is_empty() { "any" } else { t }
        }
        None => "any",
    }
}

/// Does storing `target` (a top-level FnDecl face) into a slot
/// annotated `ann` leave the bare indirect call reading registers
/// the caller never filled — i.e. are the two faces NOT
/// spelling-identical? `false` when `ann` is not a splittable
/// fn-type (nothing to compare against — the slot's own lane
/// answers) or the target is rest/generator-shaped (other lanes own
/// those).
fn store_mismatches(params: &[Param], return_type: Option<&str>, ann: &str) -> bool {
    if params.iter().any(|p| p.is_rest) {
        return false;
    }
    let Some(canon) = crate::num_width::fn_type_canon(ann) else {
        return false;
    };
    let Some((formal_ps, formal_ret)) = crate::num_width::split_fn_type(&canon) else {
        return false;
    };
    let formal_ps: Vec<&str> = formal_ps.into_iter().filter(|p| !p.is_empty()).collect();
    let user: Vec<&str> = params
        .iter()
        .filter(|p| !p.name.starts_with("__"))
        .map(|p| norm_ann(p.type_ann.as_deref()))
        .collect();
    let target_ret = return_type.map(str::trim).unwrap_or("void");
    user.len() != formal_ps.len()
        || user.iter().zip(formal_ps.iter()).any(|(a, f)| a != f)
        || target_ret != formal_ret
}

/// A function's USER face as comparable spellings: the params the
/// caller actually fills, plus the return type (`void` when absent —
/// the same default the annotation side reads).
///
/// `None` for a rest-shaped face: rest rides its own variadic route,
/// so its calls never take the bare indirect lane this census
/// guards.
fn user_face<'a>(params: &'a [Param], ret: Option<&'a str>) -> Option<(Vec<&'a str>, &'a str)> {
    if params.iter().any(|p| p.is_rest) {
        return None;
    }
    Some((
        params
            .iter()
            .filter(|p| !p.name.starts_with("__"))
            .map(|p| norm_ann(p.type_ann.as_deref()))
            .collect(),
        ret.map(str::trim).unwrap_or("void"),
    ))
}

/// The top-level FnDecl name a store's rhs names, for either
/// spelling of "a function value written into this slot".
///
/// Rotation 465 — the census read a bare `Expr::Ident` only, which is
/// the spelling for `slot = gb`. A FUNCTION LITERAL is the other one:
/// `lift_arrow_fns` (which runs before this pass) has already moved
/// the body to a top-level FnDecl and left an `Expr::Closure` naming
/// it, so its face is in `fn_sigs` under `fn_name` and the same
/// comparison applies verbatim. Missing it meant `let slot: () =>
/// void = ga; slot = function (p = 5) {…}; slot();` took the bare
/// indirect call shaped by the annotation and the callee read an
/// argument register the caller never filled.
fn store_face_name(ast: &Ast, e: super::ExprId) -> Option<&str> {
    match ast.get_expr(e) {
        Expr::Ident(n) => Some(n.as_str()),
        Expr::Closure { fn_name, .. } => Some(fn_name.as_str()),
        _ => None,
    }
}

/// See module doc. Walks every MUTABLE fn-type-annotated `LetDecl`
/// (at any nesting depth) plus every `Expr::Assign` in the arena
/// whose target is such a binding's name, and records the binding
/// when a store's rhs is a known FnDecl Ident with a mismatched
/// face. Name-keyed like `variadic_value_bindings` — a same-named
/// sig-exact binding elsewhere takes the boxed lane too (slower,
/// behavior-identical).
pub(super) fn collect_fnsig_mismatch_bindings(
    ast: &Ast,
    fn_sigs: &HashMap<String, (Vec<Param>, Option<String>, crate::lexer::Span)>,
) -> HashSet<String> {
    let mut gen_fns: HashSet<&str> = HashSet::new();
    for s in &ast.stmts {
        if let Stmt::FnDecl {
            name,
            is_generator: true,
            ..
        } = s
        {
            gen_fns.insert(name);
        }
    }
    // `snapshot_fn_sigs` drops every CLOSURE-shaped FnDecl (an
    // `__env` first param) — a lifted function literal is one, and it
    // is precisely what `store_face_name` resolves a store rhs to.
    // Its user face is the same list minus the hidden `__`-prefixed
    // params, which `store_mismatches` already filters, so the faces
    // merge into one lookup.
    let mut closure_faces: HashMap<&str, (&[Param], Option<&str>)> = HashMap::new();
    for s in &ast.stmts {
        if let Stmt::FnDecl {
            name,
            params,
            return_type,
            ..
        } = s
            && params.first().is_some_and(|p| p.name == "__env")
        {
            closure_faces.insert(name, (params.as_slice(), return_type.as_deref()));
        }
    }
    let mismatches = |gname: &str, ann: &str| -> bool {
        if gen_fns.contains(gname) || ast.async_generator_fns.contains(gname) {
            return false;
        }
        if let Some((ps, ret, _)) = fn_sigs.get(gname) {
            return store_mismatches(ps, ret.as_deref(), ann);
        }
        closure_faces
            .get(gname)
            .is_some_and(|(ps, ret)| store_mismatches(ps, *ret, ann))
    };
    // The same two lookups as one accessor, for the face-to-face
    // comparison below (which has no annotation string to compare
    // against).
    let face_of = |gname: &str| -> Option<(Vec<&str>, &str)> {
        if gen_fns.contains(gname) || ast.async_generator_fns.contains(gname) {
            return None;
        }
        match fn_sigs.get(gname) {
            Some((ps, ret, _)) => user_face(ps, ret.as_deref()),
            None => closure_faces
                .get(gname)
                .and_then(|(ps, ret)| user_face(ps, *ret)),
        }
    };
    // Mutable fn-typed declarations (name → ann), init sites checked
    // in the same walk.
    let mut decls: HashMap<String, String> = HashMap::new();
    let mut out: HashSet<String> = HashSet::new();
    let mut literal_lets: HashMap<&str, &str> = HashMap::new();
    collect_literal_lets(ast, &ast.stmts, &mut literal_lets);
    collect_decl_sites(
        ast,
        &ast.stmts,
        &mismatches,
        &literal_lets,
        &mut decls,
        &mut out,
    );
    // Rotation 465 — the UNANNOTATED mutable slot. Its type comes
    // from the initializer's own face, so there is no annotation
    // string to compare a later store against; the two faces compare
    // directly instead, and the consequence of a mismatch is the
    // same one (`var x = function () { return 1 }; x = function (a) {
    // return a }; x(7)` answered NaN — the bare indirect call shaped
    // by the INIT's zero-param face, with `a` read off a register the
    // caller never filled).
    let mut inferred: HashMap<&str, (Vec<&str>, &str)> = HashMap::new();
    collect_inferred_sites(ast, &ast.stmts, &face_of, &mut inferred);
    // Assign sites over the whole arena — scope-free name matching,
    // the conservative direction (see fn doc).
    for e in &ast.exprs {
        let Expr::Assign { target, value } = e else {
            continue;
        };
        let Expr::Ident(tname) = ast.get_expr(*target) else {
            continue;
        };
        let Some(gname) = store_face_name(ast, *value) else {
            continue;
        };
        if let Some(ann) = decls.get(tname)
            && mismatches(gname, ann)
        {
            out.insert(tname.clone());
        }
        if let Some(init_face) = inferred.get(tname.as_str())
            && face_of(gname).is_none_or(|f| f != *init_face)
        {
            out.insert(tname.clone());
        }
    }
    out
}

/// The unannotated-mutable-slot walk — same nested spine as
/// [`collect_decl_sites`], recording each such binding's INIT face so
/// the assign loop can compare later stores against it. A slot whose
/// init is not a resolvable function face is not in the map at all,
/// and one whose init face is rest-shaped stays out too (that route
/// is variadic already).
/// A function face: (param annotations, return annotation).
type Face<'a> = (Vec<&'a str>, &'a str);

fn collect_inferred_sites<'a>(
    ast: &'a Ast,
    stmts: &'a [Stmt],
    face_of: &dyn Fn(&str) -> Option<Face<'a>>,
    out: &mut HashMap<&'a str, Face<'a>>,
) {
    for s in stmts {
        if let Stmt::LetDecl {
            mutable: true,
            type_ann: None,
            name,
            init,
            ..
        } = s
            && let Some(gname) = store_face_name(ast, *init)
            && let Some(face) = face_of(gname)
        {
            out.insert(name, face);
        }
        super::stmt_nested_lists::for_each_nested_list(s, &mut |inner| {
            collect_inferred_sites(ast, inner, face_of, out)
        });
    }
}

/// The nested-LetDecl walk (the `sig_thunk::collect_let_sites`
/// recursion shape over the shared nested-statement spine).
fn collect_decl_sites(
    ast: &Ast,
    stmts: &[Stmt],
    mismatches: &dyn Fn(&str, &str) -> bool,
    literal_lets: &HashMap<&str, &str>,
    decls: &mut HashMap<String, String>,
    out: &mut HashSet<String>,
) {
    for s in stmts {
        // `var` counts (rotation 465). The mismatch is a property of
        // the SLOT's annotation against the stored face, and `var`
        // says nothing about either — a top-level `var slot: () =>
        // void = ga; slot = gb;` printed `gb [unknown-any-tag]`,
        // the callee reading the register the caller never filled,
        // exactly the answer this census exists to prevent. (A
        // nested `var` reaches here as an any-typed hoist prelude
        // and fails `is_fn_like_ann` on its own.)
        if let Stmt::LetDecl {
            mutable: true,
            type_ann: Some(ann),
            name,
            init,
            ..
        } = s
            && super::lift_arrow_fns::is_fn_like_ann(ann)
        {
            decls.insert(name.clone(), ann.clone());
            if let Some(gname) = store_face_name(ast, *init)
                && mismatches(gname, ann)
            {
                out.insert(name.clone());
            }
        }
        // An immutable binding whose init is a function literal
        // declaring a parameter type other than the annotation's
        // (`const h: (s: any) => number = (s: string) => s.length`,
        // legal because `any` is assignable to `string`): the bare
        // indirect call passes the annotation's representation into
        // a body that reads its own, so its calls ride the boxed
        // entry too, whose adapter hands a non-fitting argument to
        // the literal's any twin. The return face is contextual (the
        // literal takes the annotation's), so only parameters
        // compare. The init may name a binding of such a literal
        // (scope-free, the conservative direction); a named-function
        // init has its sig-thunk instead.
        if let Stmt::LetDecl {
            mutable: false,
            type_ann: Some(ann),
            name,
            init,
            ..
        } = s
            && super::lift_arrow_fns::is_fn_like_ann(ann)
            && let Some(fn_name) = match ast.get_expr(*init) {
                Expr::Closure { fn_name, .. } => Some(fn_name.as_str()),
                Expr::Ident(n) => literal_lets.get(n.as_str()).copied(),
                _ => None,
            }
            && literal_params_mismatch(ast, fn_name, ann)
        {
            out.insert(name.clone());
        }
        super::stmt_nested_lists::for_each_nested_list(s, &mut |inner| {
            collect_decl_sites(ast, inner, mismatches, literal_lets, decls, out)
        });
    }
}

/// The fn-typed parameters some call fills with a function literal
/// declaring a parameter the annotation does not, or one annotated
/// otherwise (`function apply(f: (s: any) => number) { … }` called as
/// `apply((s: string) => s.length)`): function name → parameter names.
/// Their calls ride the boxed entry, like a mismatched binding's
/// (the parameter is a closure cell once a literal flows in,
/// `ast_closure_param_tag`).
pub(super) fn collect_fn_param_mismatches(ast: &Ast) -> HashMap<String, HashSet<String>> {
    let mut literal_lets: HashMap<&str, &str> = HashMap::new();
    collect_literal_lets(ast, &ast.stmts, &mut literal_lets);
    let mut decls: HashMap<&str, &[Param]> = HashMap::new();
    for s in &ast.stmts {
        if let Stmt::FnDecl { name, params, .. } = s {
            decls.insert(name, params);
        }
    }
    let mut out: HashMap<String, HashSet<String>> = HashMap::new();
    for e in &ast.exprs {
        let Expr::Call { callee, args } = e else {
            continue;
        };
        let Expr::Ident(g) = ast.get_expr(*callee) else {
            continue;
        };
        let Some(params) = decls.get(g.as_str()) else {
            continue;
        };
        let user: Vec<&Param> = params
            .iter()
            .filter(|p| !p.name.starts_with("__"))
            .collect();
        for (arg, p) in args.iter().zip(user) {
            let literal = match ast.get_expr(*arg) {
                Expr::Closure { fn_name, .. } => Some(fn_name.as_str()),
                Expr::Ident(n) => literal_lets.get(n.as_str()).copied(),
                _ => None,
            };
            if let (Some(fn_name), Some(ann)) = (literal, p.type_ann.as_deref())
                && !p.is_rest
                && super::lift_arrow_fns::is_fn_like_ann(ann)
                && literal_params_mismatch(ast, fn_name, ann)
            {
                out.entry(g.clone()).or_default().insert(p.name.clone());
            }
        }
    }
    out
}

/// The bindings initialized from a function literal, at any depth:
/// binding name → the literal's lifted name.
fn collect_literal_lets<'a>(ast: &'a Ast, stmts: &'a [Stmt], out: &mut HashMap<&'a str, &'a str>) {
    for s in stmts {
        if let Stmt::LetDecl { name, init, .. } = s
            && let Expr::Closure { fn_name, .. } = ast.get_expr(*init)
        {
            out.insert(name, fn_name);
        }
        super::stmt_nested_lists::for_each_nested_list(s, &mut |inner| {
            collect_literal_lets(ast, inner, out)
        });
    }
}

/// Does the lifted function literal `fn_name` declare a user
/// parameter the fn-type `ann` does not, or one annotated otherwise?
fn literal_params_mismatch(ast: &Ast, fn_name: &str, ann: &str) -> bool {
    let Some(params) = ast.stmts.iter().find_map(|s| match s {
        Stmt::FnDecl { name, params, .. } if name == fn_name => Some(params),
        _ => None,
    }) else {
        return false;
    };
    let Some((user, _)) = user_face(params, None) else {
        return false;
    };
    let Some(canon) = crate::num_width::fn_type_canon(ann) else {
        return false;
    };
    let Some((formal_ps, _)) = crate::num_width::split_fn_type(&canon) else {
        return false;
    };
    let formal_ps: Vec<&str> = formal_ps.into_iter().filter(|p| !p.is_empty()).collect();
    // a literal declaring more parameters than the annotation reads
    // registers the caller never filled
    user.len() > formal_ps.len() || formal_ps.iter().zip(user).any(|(f, a)| *f != a)
}
