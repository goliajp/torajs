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
    let mismatches = |gname: &str, ann: &str| -> bool {
        !gen_fns.contains(gname)
            && !ast.async_generator_fns.contains(gname)
            && fn_sigs
                .get(gname)
                .is_some_and(|(ps, ret, _)| store_mismatches(ps, ret.as_deref(), ann))
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
            && let Expr::Ident(gname) = ast.get_expr(*value)
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
        if let Stmt::LetDecl {
            mutable: true,
            type_ann: Some(ann),
            name,
            init,
            is_var: false,
            ..
        } = s
            && super::lift_arrow_fns::is_fn_like_ann(ann)
        {
            decls.insert(name.clone(), ann.clone());
            if let Expr::Ident(gname) = ast.get_expr(*init)
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
