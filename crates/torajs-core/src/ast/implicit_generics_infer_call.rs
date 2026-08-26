//! The `Expr::Call` arm of the return sniff — split from
//! [`super::implicit_generics_infer`] (file-size limit): the parent
//! answers "what type does this expression have", this answers the
//! one arm that consults the published fn sigs (and the parser's
//! synthetic relational calls) for a callee.

use super::{AstExprsView, Expr, ExprId, Param, infer_expr_ann_with};

/// Type of `callee(args)` — see [`infer_expr_ann_with`].
pub(super) fn infer_call_ann(
    exprs: AstExprsView,
    callee: ExprId,
    args: &[ExprId],
    params: &[Param],
    binds: &std::collections::HashMap<String, String>,
    fn_sigs: &std::collections::HashMap<String, String>,
) -> Option<String> {
    let recur = |id: ExprId| infer_expr_ann_with(exprs, id, params, binds, fn_sigs);
    let callee = &callee;
    let c = exprs.get(callee.0 as usize)?;
    if let Expr::Ident(n) = c {
        // The parser's synthetic relational calls answer Bool
        // (`in` / the §13.10 `#x in o` brand check) — they
        // are not user fns, so fn_sigs can't know them, and a
        // bare `return #x in o` would bail the sniff to Void
        // (the mono body then hands unboxed garbage back
        // through an any ret).
        if n == "__torajs_in_op" || n == "__torajs_priv_in_op" {
            return Some("boolean".to_string());
        }
        return fn_sigs.get(n).cloned();
    }
    if let Expr::Member { obj, name } = c {
        let r = recur(*obj)?;
        let elem = r.strip_suffix("[]");
        let ret: &str = match (r.as_str(), elem, name.as_str()) {
            (
                "string",
                _,
                "toUpperCase" | "toLowerCase" | "trim" | "trimStart" | "trimEnd" | "slice"
                | "substring" | "repeat" | "concat" | "replace" | "replaceAll" | "padStart"
                | "padEnd" | "at" | "charAt",
            ) => "string",
            ("string", _, "startsWith" | "endsWith" | "includes") => "boolean",
            ("string", _, "indexOf" | "lastIndexOf" | "charCodeAt") => "number",
            // RFC 20260705 chunk 557 — conversion methods on known
            // primitive/array receivers (`j.toString()` inside a
            // string-concat chain bailed the whole sniff → Void).
            // Receivers whose ann we can't resolve still bail —
            // user classes may override toString with another shape.
            ("number" | "boolean" | "string", _, "toString" | "toLocaleString") => "string",
            ("number", _, "toFixed" | "toPrecision" | "toExponential") => "string",
            (_, Some(_), "toString" | "toLocaleString") => "string",
            (_, Some(e), "pop" | "shift" | "at" | "find" | "findLast") => e,
            // 403-01 — a map/flatMap result element is the
            // CALLBACK's return (doc on the sibling); an
            // opaque callback keeps the historical same-`T`
            // approximation.
            (_, Some(_), "map" | "flatMap") => {
                match super::implicit_generics_cb_ret::hof_result_ann(
                    name, exprs, args, params, binds, fn_sigs,
                ) {
                    Some(u) => return Some(u),
                    None => &r,
                }
            }
            (_, Some(_), "slice" | "reverse" | "sort" | "concat" | "fill" | "filter" | "flat") => {
                &r
            }
            (_, Some(_), "every" | "some" | "includes") => "boolean",
            (_, Some(_), "indexOf" | "lastIndexOf" | "findIndex" | "findLastIndex") => "number",
            (_, Some(_), "join") => "string",
            // `reduce` / `reduceRight` return type is the callback's
            // accumulator type — for the common case where the
            // accumulator is the same shape as elements (e.g.
            // `xs.reduce((a, b) => a + b, 0)` on Number[]), it
            // matches the element type. Mixed-type accumulators
            // (cb returns a different shape than elem) still need
            // explicit ret annotation (substrate follow-up).
            (_, Some(e), "reduce" | "reduceRight") => e,
            _ => return None,
        };
        return Some(ret.to_string());
    }
    None
}
