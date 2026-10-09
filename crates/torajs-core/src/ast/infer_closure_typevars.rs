//! The user-fn closure-hint arm of [`super::infer_closure_params`]
//! and its call-site typevar resolution.
//!
//! A generic callee's param fn-type spelling may mention its type
//! params (`g<T>(cb: (...args: T[]) => T, x: T)`); projecting a raw
//! `T` onto a lifted closure trips `build_fn_type` with "unknown
//! return type `T`" — the typevar is out of scope there. These two
//! helpers let the hint arm substitute call-site-pinned typevars
//! first and skip any spelling that still mentions an unresolved
//! one.

use std::collections::{HashMap, HashSet};

use super::infer_closure_params::infer_lit_ann;
use super::infer_closure_params_helpers::seed_container_arg_hints;
use super::scalar_alias_spell::type_words;
use super::{Ast, ExprId, Stmt};
use crate::num_width::{fn_type_canon, split_fn_type};
use crate::ssa_lower_generics_monomorph::substitute_in_ann;

/// A param whose annotation is exactly one bare typevar name and
/// whose arg is a literal shape pins that typevar to the literal's
/// annotation (`x: T` + `21` → `T` → "number"). A type argument the
/// call writes out (`g<number>(..)`) pins its parameter before any of
/// that, unless it names a type parameter some declaration binds
/// (`hof<U>(..)` inside `outer<U>`): that is the caller's own
/// parameter, out of scope in the lifted closure the hint lands on.
/// Anything fancier (typed idents, nested generic positions,
/// closure-param back-inference) is left unresolved — the caller
/// projects no spelling that still mentions an unresolved typevar.
pub(super) fn resolve_call_site_typevars(
    ast: &Ast,
    call: ExprId,
    type_params: &[String],
    pos_anns: &[Option<String>],
    args: &[ExprId],
) -> Vec<(String, String)> {
    let mut subst: Vec<(String, String)> = match ast.call_type_args.get(&call) {
        Some(written) => {
            let bound = bound_type_params(ast);
            type_params
                .iter()
                .zip(written)
                .filter(|(_, a)| !type_words(a).any(|w| bound.contains(w)))
                .map(|(tp, a)| (tp.clone(), a.clone()))
                .collect()
        }
        None => Vec::new(),
    };
    for (i, arg) in args.iter().enumerate() {
        let Some(Some(pann)) = pos_anns.get(i) else {
            continue;
        };
        let pann = pann.trim();
        if !type_params.iter().any(|tp| tp == pann) {
            continue;
        }
        if subst.iter().any(|(from, _)| from == pann) {
            continue;
        }
        if let Some(lit_ann) = infer_lit_ann(ast, *arg) {
            subst.push((pann.to_string(), lit_ann));
        }
    }
    subst
}

/// Every type parameter name a declaration in the program binds.
fn bound_type_params(ast: &Ast) -> HashSet<&str> {
    let mut out = HashSet::new();
    for s in &ast.stmts {
        match s {
            Stmt::FnDecl { type_params, .. } | Stmt::TypeDecl { type_params, .. } => {
                out.extend(type_params.iter().map(String::as_str));
            }
            Stmt::ClassDecl {
                type_params,
                methods,
                static_methods,
                ..
            } => {
                out.extend(type_params.iter().map(String::as_str));
                for m in methods.iter().chain(static_methods.iter()) {
                    out.extend(m.type_params.iter().map(String::as_str));
                }
            }
            _ => {}
        }
    }
    out
}

/// True when `ann` contains any of `words` as a whole word (word
/// boundary = non-alphanumeric, non-`_` — the same rule
/// `substitute_in_ann` scans with).
pub(super) fn mentions_any_word(ann: &str, words: &[String]) -> bool {
    let bytes = ann.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let c = bytes[i];
        if !(c.is_ascii_alphabetic() || c == b'_') {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
            i += 1;
        }
        let word = &ann[start..i];
        if words.iter().any(|w| w == word) {
            return true;
        }
    }
    false
}

/// Where the user-fn hint arm writes: full (params + return) seeds,
/// and parameter-only seeds.
pub(super) struct UserFnHintSinks<'a> {
    pub(super) updates: &'a mut HashMap<String, (Vec<String>, String)>,
    pub(super) param_only: &'a mut HashMap<String, Vec<String>>,
}

/// User-fn callee arm — an `__fn(P|..)->(R)`-annotated param at a closure
/// arg position hints the lifted closure's own param/ret annotations:
/// an unannotated arrow handed to a user function takes the parameter
/// and return types that function declares for that slot.
///
/// Generic callee — the hint spellings may mention the
/// callee's type params (`g<T>(cb: (...args: T[]) => T, x: T)`);
/// projecting a raw `T` onto the lifted closure trips `build_fn_type`
/// with "unknown return type `T`" (the typevar is out of scope there).
/// Resolve typevars from the same call site first (a bare-typevar param
/// position holding a literal arg pins it: `x: T` + `21` → T=number).
/// A spelling still mentioning an unresolved typevar after substitution
/// is not projected, but the others still are: a parameter's contextual
/// type does not wait on a type parameter only the return mentions
/// (a contextually typed function expression in TS — `hof<T>(f: (n:
/// number) => T)` types `n` as `number` and infers `T` from the body).
/// Parameters are projected up to the first one that mentions an
/// unresolved typevar; with the return unresolved, only parameters.
pub(super) fn apply_user_fn_callee_hint(
    ast: &Ast,
    call: ExprId,
    args: &[ExprId],
    closure_args: &[(usize, String)],
    pos_anns: &[Option<String>],
    tps: Option<&[String]>,
    sinks: UserFnHintSinks<'_>,
) {
    let subst = tps.map(|tps| resolve_call_site_typevars(ast, call, tps, pos_anns, args));
    // A container literal at an argument position carries that
    // position's declared type inward; the arrows inside it are not
    // themselves the argument.
    for (arg_idx, arg) in args.iter().enumerate() {
        if let Some(Some(pann)) = pos_anns.get(arg_idx) {
            seed_container_arg_hints(ast, pann, *arg, sinks.updates);
        }
    }
    let subst = subst.as_deref();
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
        let mut param_anns = subst_anns(&param_spellings, subst);
        let ret_ann = subst_ann(ret_spelling, subst);
        if let Some(tps) = tps {
            let open_param = param_anns.iter().position(|a| mentions_any_word(a, tps));
            if open_param.is_some() || mentions_any_word(&ret_ann, tps) {
                param_anns.truncate(open_param.unwrap_or(param_anns.len()));
                if !param_anns.is_empty() {
                    sinks.param_only.insert(fn_name.clone(), param_anns);
                }
                continue;
            }
        }
        sinks.updates.insert(fn_name.clone(), (param_anns, ret_ann));
    }
}

/// `ann` with the call site's typevar bindings substituted, when the
/// callee is generic.
fn subst_ann(ann: &str, subst: Option<&[(String, String)]>) -> String {
    subst.map_or_else(|| ann.to_string(), |s| substitute_in_ann(ann, s))
}

fn subst_anns(anns: &[&str], subst: Option<&[(String, String)]>) -> Vec<String> {
    anns.iter().map(|a| subst_ann(a, subst)).collect()
}
