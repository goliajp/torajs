//! The `fn_sigs` map the generator prep's let-lift consults when a
//! local is initialized by a call — every top-level function's
//! DECLARED return type, keyed by name — moved out of
//! `desugar_generators_field_ann` (which had reached the 500-line HARD
//! limit) as the family that answers "what does calling this name
//! give" rather than "what is this initializer".

use super::{Ast, Expr, Stmt};

/// Every top-level function's *declared* return type, keyed by name —
/// the `fn_sigs` map D0's let-lift consults when a generator local is
/// initialized by a call.
///
/// Declared, not inferred: this pass runs well before
/// `desugar_implicit_generics`, so an inferred signature does not
/// exist yet. A call to a function that never wrote its return type
/// is simply absent from the map, and the lift falls back exactly as
/// it did before.
///
/// A `function*` is the exception, and the reason is that its
/// declared return type describes what it YIELDS, not what calling it
/// answers. `function* ag(): any` called as `ag()` answers the
/// iterator object — which this very pass is about to mint as
/// `__Gen_ag` — so that is what the map says. Reading the annotation
/// instead pinned `const it = ag()` to the yield type and every
/// `it.next()` after it failed ("no member `.next` on type Number").
pub(super) fn collect_declared_fn_return_types(
    ast: &Ast,
) -> std::collections::HashMap<String, String> {
    ast.stmts
        .iter()
        .filter_map(|s| match s {
            Stmt::FnDecl {
                name,
                is_generator: true,
                ..
            } => Some((name.clone(), format!("__Gen_{name}"))),
            // A return naming the function's own type parameter is
            // settled per call, after this pass: written into a field
            // as is, `T` names nothing there ("unknown type `T` for
            // field"), so the slot holds whatever the call answers.
            Stmt::FnDecl {
                name,
                type_params,
                return_type: Some(rt),
                ..
            } if names_type_param(rt, type_params) => Some((name.clone(), "any".to_string())),
            Stmt::FnDecl {
                name,
                return_type: Some(rt),
                ..
            } => Some((name.clone(), normalize_void(rt))),
            // A function with no value return answers `undefined`
            // (§14.10) whether or not it wrote `: void` — the same
            // rule `preinfer_closure_sigs` applies to a lifted
            // closure. Without it `const a = sideEffectOnly()` took
            // the `number` fallback and would not compile ("field is
            // Number, value is Undefined").
            Stmt::FnDecl { name, body, .. } if !super::body_has_value_return(body) => {
                Some((name.clone(), "any".to_string()))
            }
            // An UNANNOTATED function with a value return: JS's
            // untyped slot is `any` — the same answer `undefined` /
            // `null` initializers get in `direct_field_ann`, and for
            // the same reason. Absent from the map, the call declined
            // and the field took the `number` fallback, which is a
            // CLAIM about the value: a function answering a symbol
            // (the t262 forbidden-ext `inner()` shape) had its result
            // stored through a number slot and surfaced as a raw
            // pointer error.
            Stmt::FnDecl { name, .. } => Some((name.clone(), "any".to_string())),
            // 551-02 — a top-level `const s = (n): T => …` binding is
            // called exactly like a declared fn, and its arrow's
            // DECLARED return type answers the same question (this
            // pass runs before lift_arrow_fns, so the init is still
            // `Expr::ArrowFn`). Same declared-not-inferred rule: an
            // unannotated arrow answers `any`. Absent from the map,
            // the call declined and `const t = s(k)` across a yield
            // took the `number` fallback — a String value through a
            // Number field.
            Stmt::LetDecl { name, init, .. } => match ast.get_expr(*init) {
                Expr::ArrowFn {
                    return_type: Some(rt),
                    ..
                } => Some((name.clone(), normalize_void(rt))),
                Expr::ArrowFn { .. } => Some((name.clone(), "any".to_string())),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

fn names_type_param(rt: &str, type_params: &[String]) -> bool {
    rt.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '$'))
        .any(|w| type_params.iter().any(|t| t == w))
}

/// `void` as a FIELD annotation, which is what a lifted local becomes:
/// `any`. A field has to be able to hold the value, and the value a
/// void call produces is `undefined` — which is what an `any` slot
/// holds and a `void` slot cannot. Left as written, `const b =
/// voidCall()` yielded `null`.
fn normalize_void(rt: &str) -> String {
    if rt == "void" {
        "any".into()
    } else {
        rt.into()
    }
}
