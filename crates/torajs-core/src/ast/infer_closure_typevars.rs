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

use std::collections::HashMap;

use super::infer_closure_params::infer_lit_ann;
use super::infer_closure_params_helpers::seed_container_arg_hints;
use super::{Ast, ExprId};
use crate::num_width::{fn_type_canon, split_fn_type};

/// A param whose annotation is exactly one bare typevar name and
/// whose arg is a literal shape pins that typevar to the literal's
/// annotation (`x: T` + `21` → `T` → "number"). Anything fancier
/// (typed idents, nested generic positions, closure-param
/// back-inference) is left unresolved — the caller skips projecting
/// any spelling that still mentions an unresolved typevar.
pub(super) fn resolve_call_site_typevars(
    ast: &Ast,
    type_params: &[String],
    pos_anns: &[Option<String>],
    args: &[ExprId],
) -> Vec<(String, String)> {
    let mut subst: Vec<(String, String)> = Vec::new();
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
    // A container literal at an argument position carries that
    // position's declared type inward; the arrows inside it are not
    // themselves the argument.
    for (arg_idx, arg) in args.iter().enumerate() {
        if let Some(Some(pann)) = pos_anns.get(arg_idx) {
            seed_container_arg_hints(ast, pann, *arg, updates);
        }
    }
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
