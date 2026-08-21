//! 424-04 — mismatch census for MUTABLE fn-typed bindings.
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
    // Mutable fn-typed declarations (name → ann), init sites checked
    // in the same walk.
    let mut decls: HashMap<String, String> = HashMap::new();
    let mut out: HashSet<String> = HashSet::new();
    collect_decl_sites(ast, &ast.stmts, &mismatches, &mut decls, &mut out);
    // Assign sites over the whole arena — scope-free name matching,
    // the conservative direction (see fn doc).
    for e in &ast.exprs {
        if let Expr::Assign { target, value } = e
            && let Expr::Ident(tname) = ast.get_expr(*target)
            && let Some(ann) = decls.get(tname)
            && let Some(gname) = store_face_name(ast, *value)
            && mismatches(gname, ann)
        {
            out.insert(tname.clone());
        }
    }
    out
}

/// The nested-LetDecl walk (the `sig_thunk::collect_let_sites`
/// recursion shape over the shared nested-statement spine).
fn collect_decl_sites(
    ast: &Ast,
    stmts: &[Stmt],
    mismatches: &dyn Fn(&str, &str) -> bool,
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
        super::stmt_nested_lists::for_each_nested_list(s, &mut |inner| {
            collect_decl_sites(ast, inner, mismatches, decls, out)
        });
    }
}
