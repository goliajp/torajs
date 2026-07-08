//! Call-site typevar resolution for the user-fn closure-hint arm of
//! [`super::infer_closure_params`] (chunk 682).
//!
//! A generic callee's param fn-type spelling may mention its type
//! params (`g<T>(cb: (...args: T[]) => T, x: T)`); projecting a raw
//! `T` onto a lifted closure trips `build_fn_type` with "unknown
//! return type `T`" — the typevar is out of scope there. These two
//! helpers let the hint arm substitute call-site-pinned typevars
//! first and skip any spelling that still mentions an unresolved
//! one.

use super::infer_closure_params::infer_lit_ann;
use super::{Ast, ExprId};

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
