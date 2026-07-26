//! Support helpers for [`super::infer_closure_params`] — extracted
//! as the rotation-196 file-size sweep. The parent had drifted to
//! 508 prod LOC over the recent .map/.flatMap wedge additions;
//! `mapset_foreach_expected` (Map/Set forEach param shape) and
//! `build_ann_table` (per-name annotation gather over top-level
//! FnDecls + let-inits) are self-contained and share zero mutable
//! state with the main walker, so cluster naturally into a sibling.
//! Verbatim moves; signatures / semantics unchanged.
//!
//! Rotation 197 sweep — `collect_fn_decl_metadata` (four side-channel
//! HashMap gather), `apply_user_fn_callee_hint` (chunk-554 face ②
//! user-fn callee arm), and `apply_hof_param_only_arm` (map / flatMap /
//! reduce / reduceRight param-only seed) also moved here to bring the
//! parent's `infer_anonymous_closure_params` fn under the 200-line hard
//! limit without breaking the 500-line file cap.

use std::collections::HashMap;

use super::infer_closure_params::infer_lit_ann;
use super::infer_closure_params_apply::body_returns_value;
use super::infer_closure_typevars::{mentions_any_word, resolve_call_site_typevars};
use super::infer_return_ann;
use crate::ast::infer_closure_lets::{collect_let_anns, collect_let_init_anns};
use crate::ast::{Ast, Expr, ExprId, Stmt};
use crate::num_width::{fn_type_canon, split_fn_type};

/// Callback param/return annotations for `forEach` on a `Map<K|V>` /
/// `Set<T>` receiver ann (the flat generic spelling). None for any
/// other method or receiver shape — Map/Set carry no other
/// callback-bearing methods.
pub(super) fn mapset_foreach_expected(ann: &str, method: &str) -> Option<(Vec<String>, String)> {
    if method != "forEach" {
        return None;
    }
    if let Some(inner) = ann.strip_prefix("Map<").and_then(|r| r.strip_suffix('>')) {
        let parts = crate::check_type_ann::split_top_pipe(inner, true);
        let [k, v] = parts.as_slice() else {
            return None;
        };
        return Some((
            vec![v.to_string(), k.to_string(), ann.to_string()],
            "void".into(),
        ));
    }
    if let Some(inner) = ann.strip_prefix("Set<").and_then(|r| r.strip_suffix('>')) {
        let parts = crate::check_type_ann::split_top_pipe(inner, true);
        let [t] = parts.as_slice() else {
            return None;
        };
        return Some((
            vec![t.to_string(), t.to_string(), ann.to_string()],
            "void".into(),
        ));
    }
    None
}

/// Per-name → type-annotation table feeding receiver resolution.
/// Walk all top-level FnDecl bodies gathering param + let-decl
/// annotations (the same name may appear in multiple fns; call-site
/// inference resolves the right binding via the enclosing fn), plus:
///
/// - V3-18 m1.h.23 — top-level let decls (the synthetic `main`
///   wraps these at ssa_lower time, but at this AST pass they sit at
///   ast.stmts level, so the FnDecl-only walk misses them; without
///   this `let arr = [1,2,3]; arr.find(x => ...)` can't infer x).
/// - Inferred-from-init shape: `let arr = [<lit>, ...]` infers
///   arr's annotation as `<lit_ty>[]` so .map / .filter on
///   unannotated lets still get param inference.
/// Read `ann` as a fn-type canonical spelling, chasing bare type
/// aliases to get there: `type F = (n: number) => number` parks its RHS
/// under an `__alias__` sentinel field, so the annotation on `const g:
/// F` is the bare name `F` and canonicalizes to nothing on its own.
/// Bounded so a `type A = B; type B = A` pair cannot spin.
pub(super) fn resolve_fn_type_ann(ast: &Ast, ann: &str) -> Option<String> {
    let mut cur = ann.to_string();
    for _ in 0..8 {
        if let Some(canon) = fn_type_canon(&cur) {
            return Some(canon);
        }
        cur = ast.stmts.iter().find_map(|s| match s {
            Stmt::TypeDecl { name, fields, .. } if *name == cur => fields
                .iter()
                .find(|(f, _)| f == "__alias__")
                .map(|(_, a)| a.clone()),
            _ => None,
        })?;
    }
    None
}

/// Project each `lifted closure → binding annotation` hint onto the
/// closure's params and return, the let-init counterpart of
/// [`apply_user_fn_callee_hint`]. Both land in the same `updates` map
/// and go through the same applier, which is what makes an explicitly
/// annotated param win, a shorter param list not overrun, and a
/// void-bodied closure keep its Void return.
pub(super) fn seed_let_ann_hints(
    ast: &Ast,
    closure_let_hints: &HashMap<String, String>,
    updates: &mut HashMap<String, (Vec<String>, String)>,
) {
    for (fn_name, ann) in closure_let_hints {
        let Some(canon) = resolve_fn_type_ann(ast, ann) else {
            continue;
        };
        let Some((param_spellings, ret_spelling)) = split_fn_type(&canon) else {
            continue;
        };
        updates.insert(
            fn_name.clone(),
            (
                param_spellings.iter().map(|s| s.to_string()).collect(),
                ret_spelling.to_string(),
            ),
        );
    }
}

pub(super) fn build_ann_table(ast: &Ast) -> (HashMap<String, String>, HashMap<String, String>) {
    let mut all_anns: HashMap<String, String> = HashMap::new();
    // Lifted-closure name → the fn-type annotation of the binding it
    // initializes, for contextual param typing (see `collect_let_anns`).
    let mut closure_hints: HashMap<String, String> = HashMap::new();
    for s in &ast.stmts {
        if let Stmt::FnDecl {
            params,
            body,
            return_type,
            ..
        } = s
        {
            for p in params {
                if let Some(ann) = &p.type_ann {
                    all_anns.insert(p.name.clone(), ann.clone());
                }
            }
            collect_let_anns(ast, body, &mut all_anns, &mut closure_hints);
            if let Some(ret_ann) = return_type {
                super::infer_closure_lets::seed_return_hints(
                    ast,
                    body,
                    ret_ann,
                    &mut closure_hints,
                );
            }
        }
    }
    collect_let_anns(ast, &ast.stmts, &mut all_anns, &mut closure_hints);
    let mut inferred_inits: HashMap<String, String> = HashMap::new();
    collect_let_init_anns(ast, &ast.stmts, &mut inferred_inits);
    for (k, v) in inferred_inits {
        all_anns.entry(k).or_insert(v);
    }
    (all_anns, closure_hints)
}

/// Walk `ast.stmts` for FnDecls and pre-compute the four per-FnDecl side
/// channels the main call-site loop reads:
///   - `fn_param_pos_anns`: name → positional param annotations (chunk-554
///     user-fn callee hint arm).
///   - `fn_type_params`: name → generic type params (chunk-682 hint typevar
///     resolution).
///   - `fn_user_param_count`: name → user-param count excluding `__env`
///     (`<string>.replace(re, cb)` shape).
///   - `fn_ret_anns`: name → resolved (or sniffed) return ann (Promise
///     chain propagation for `.then(prev_cb).then(next_cb)`).
pub(super) fn collect_fn_decl_metadata(
    ast: &Ast,
) -> (
    HashMap<String, Vec<Option<String>>>,
    HashMap<String, Vec<String>>,
    HashMap<String, usize>,
    HashMap<String, Option<String>>,
) {
    let mut fn_param_pos_anns: HashMap<String, Vec<Option<String>>> = HashMap::new();
    let mut fn_type_params: HashMap<String, Vec<String>> = HashMap::new();
    let mut fn_user_param_count: HashMap<String, usize> = HashMap::new();
    let mut fn_ret_anns: HashMap<String, Option<String>> = HashMap::new();
    for s in &ast.stmts {
        if let Stmt::FnDecl {
            name,
            params,
            type_params,
            return_type,
            body,
            ..
        } = s
        {
            fn_param_pos_anns.insert(
                name.clone(),
                params.iter().map(|p| p.type_ann.clone()).collect(),
            );
            fn_user_param_count.insert(
                name.clone(),
                params.iter().filter(|p| p.name != "__env").count(),
            );
            if !type_params.is_empty() {
                fn_type_params.insert(name.clone(), type_params.clone());
            }
            let ret = return_type.clone().or_else(|| {
                if body_returns_value(body) {
                    infer_return_ann(&ast.exprs, body, params, &HashMap::new())
                } else {
                    None
                }
            });
            fn_ret_anns.insert(name.clone(), ret);
        }
    }
    (
        fn_param_pos_anns,
        fn_type_params,
        fn_user_param_count,
        fn_ret_anns,
    )
}

/// User-fn callee arm — an `__fn(P|..)->R`-annotated param at a closure
/// arg position hints the lifted closure's own param/ret annotations
/// (chunk 554 face ②).
///
/// Generic callee (chunk 682) — the hint spellings may mention the
/// callee's type params (`g<T>(cb: (...args: T[]) => T, x: T)`);
/// projecting a raw `T` onto the lifted closure trips `build_fn_type`
/// with "unknown return type `T`" (the typevar is out of scope there).
/// Resolve typevars from the same call site first (a bare-typevar param
/// position holding a literal arg pins it: `x: T` + `21` → T=number);
/// a spelling still mentioning an unresolved typevar after substitution
/// is not projected.
pub(super) fn apply_user_fn_callee_hint(
    ast: &Ast,
    fname: &str,
    args: &[ExprId],
    closure_args: &[(usize, String)],
    fn_param_pos_anns: &HashMap<String, Vec<Option<String>>>,
    fn_type_params: &HashMap<String, Vec<String>>,
    updates: &mut HashMap<String, (Vec<String>, String)>,
) {
    let Some(pos_anns) = fn_param_pos_anns.get(fname) else {
        return;
    };
    let subst = fn_type_params
        .get(fname)
        .map(|tps| resolve_call_site_typevars(ast, tps, pos_anns, args));
    let tps = fn_type_params.get(fname);
    for (arg_idx, fn_name) in closure_args {
        let Some(Some(pann)) = pos_anns.get(*arg_idx) else {
            continue;
        };
        let Some(canon) = fn_type_canon(pann) else {
            continue;
        };
        let Some((param_spellings, ret_spelling)) = split_fn_type(&canon) else {
            continue;
        };
        let mut param_anns: Vec<String> = param_spellings.iter().map(|s| s.to_string()).collect();
        let mut ret_ann = ret_spelling.to_string();
        if let (Some(subst), Some(tps)) = (&subst, tps) {
            for a in param_anns.iter_mut() {
                *a = crate::ssa_lower_generics_monomorph::substitute_in_ann(a, subst);
            }
            ret_ann = crate::ssa_lower_generics_monomorph::substitute_in_ann(&ret_ann, subst);
            if param_anns.iter().any(|a| mentions_any_word(a, tps))
                || mentions_any_word(&ret_ann, tps)
            {
                continue;
            }
        }
        updates.insert(fn_name.clone(), (param_anns, ret_ann));
    }
}

/// Array HOF param-only arms — `.map(cb)` / `.flatMap(cb)` /
/// `.reduce(cb, seed?)` / `.reduceRight(cb, seed?)` seed only the cb's
/// user params (leave return ann to the body sniff so heterogeneous
/// returns can flow through the call-site hetero-arm without
/// `check_stmt_return` rejecting them against a strapped return ann).
///
/// Returns true if `name` matched one of the four arms — caller
/// `continue`s in that case to skip the trailing per-method
/// (param, return) table below.
pub(super) fn apply_hof_param_only_arm(
    ast: &Ast,
    name: &str,
    args: &[ExprId],
    closure_args: &[(usize, String)],
    elem_ann: &str,
    all_anns: &HashMap<String, String>,
    fn_user_param_count: &HashMap<String, usize>,
    param_only_updates: &mut HashMap<String, Vec<String>>,
) -> bool {
    // `.map(cb)` — seed only the cb's param (elem), leave the return
    // annotation to the body sniff. Heterogeneous returns like
    // `numbers.map(n => n.toString())` are accepted at the call-site by
    // [`crate::check_type_of_call_arr_map_hetero`]; strapping
    // `return_type = elem_ann` here would trip `check_stmt_return` on
    // the arrow body's Str return before the call-site arm gets a chance
    // to answer `Array<String>`.
    if name == "map" {
        for (_arg_idx, fn_name) in closure_args {
            let p = fn_user_param_count.get(fn_name).copied().unwrap_or(1);
            if p >= 1 {
                param_only_updates.insert(fn_name.clone(), vec![elem_ann.to_string(); p]);
            }
        }
        return true;
    }
    // `.flatMap(cb)` — sister to `.map`. Seed only the cb's param;
    // leave the return ann to the body sniff so a heterogeneous return
    // `Array<U>` (with U != T) can flow through
    // [`crate::check_type_of_call_arr_flat_map_hetero`] without
    // check_stmt_return rejecting it against a strapped `${elem_ann}[]`.
    if name == "flatMap" {
        for (_arg_idx, fn_name) in closure_args {
            let p = fn_user_param_count.get(fn_name).copied().unwrap_or(1);
            if p >= 1 {
                param_only_updates.insert(fn_name.clone(), vec![elem_ann.to_string(); p]);
            }
        }
        return true;
    }
    // `reduce`/`reduceRight` — seed the acc param from the SEED arg's
    // static type (args[1]) when it's a literal or typed ident;
    // otherwise fall back to elem_ann for the sum/max idiom. Return ann
    // stays for the body sniff so a `(number, number) => string` reduce
    // (`[1,2,3].reduce((a: string, x) => a + x, '')`) types through
    // instead of tripping `check_stmt_return` on the Str body return.
    if name == "reduce" || name == "reduceRight" {
        let acc_ann = args
            .get(1)
            .and_then(|&a| match ast.get_expr(a) {
                Expr::Ident(n) => all_anns.get(n).cloned(),
                _ => infer_lit_ann(ast, a),
            })
            .unwrap_or_else(|| elem_ann.to_string());
        for (_arg_idx, fn_name) in closure_args {
            let p = fn_user_param_count.get(fn_name).copied().unwrap_or(2);
            let mut param_anns = vec![acc_ann.clone()];
            while param_anns.len() < p {
                param_anns.push(elem_ann.to_string());
            }
            param_only_updates.insert(fn_name.clone(), param_anns);
        }
        return true;
    }
    false
}
