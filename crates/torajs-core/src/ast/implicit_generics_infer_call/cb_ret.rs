//! 403-01 — the return-sniff's answer for a `.map(cb)` result element.
//!
//! The sniff's container-method whitelist approximated `map` as
//! same-`T` (`numbers.map(cb)` → `number[]`), which is `filter` /
//! `slice` logic — a map's result element is the CALLBACK's return.
//! The approximation pinned an enclosing fn's inferred return to the
//! receiver's element: `function run() { return [1].map(fn)[0] }` with
//! a string-returning callback checked the hetero wedge's
//! `Array<String>` against an expected `Number` (loud), and the typed
//! element read decoded a Str box as f64 (`NaN`, silent) — top level
//! was fine because no enclosing fn asked the sniff.
//!
//! Answer the callback's return ann when it is statically legible;
//! `None` keeps the historical same-`T` approximation (never a new
//! bail — an opaque callback behaves exactly as before this file).

use crate::ast::{Expr, ExprId, Param};

/// The whitelist arm bodies for `map` / `flatMap` — the callback's
/// return ann as the result element (`map`), with one array layer
/// peeled for the §23.1.3.14 depth-1 flatten (`flatMap`). `None`
/// keeps the caller's historical same-`T` approximation.
pub(super) fn hof_result_ann(
    method: &str,
    exprs: &[Expr],
    args: &[ExprId],
    params: &[Param],
    binds: &std::collections::HashMap<String, String>,
    fn_sigs: &std::collections::HashMap<String, String>,
) -> Option<String> {
    let u = args
        .first()
        .and_then(|a| callback_ret_ann(exprs, *a, params, binds, fn_sigs))?;
    match method {
        "map" => Some(format!("{u}[]")),
        "flatMap" => {
            let elem = u.strip_suffix("[]").unwrap_or(&u);
            Some(format!("{elem}[]"))
        }
        _ => None,
    }
}

/// The `reduce` / `reduceRight` result with a seed: lib.es5's third
/// overload `reduce<U>(cb: (prev: U, cur: T, i: number, a: T[]) => U,
/// init: U): U` (§23.1.3.24 returns the final accumulator, which starts
/// as the seed). `U` is the callback's annotated accumulator parameter
/// when it has one (the seed only has to fit it), else the seed's
/// type. `None` — no seed, an annotation the sniff cannot spell, or no
/// legible seed — keeps the caller's element type, the seedless
/// overloads' `T`.
pub(super) fn reduce_result_ann(
    exprs: &[Expr],
    args: &[ExprId],
    seed: Option<String>,
    fn_sigs: &std::collections::HashMap<String, String>,
) -> Option<String> {
    if args.len() < 2 {
        return None;
    }
    let acc = match exprs.get(args[0].0 as usize)? {
        Expr::Closure { fn_name, .. } => fn_sigs.get(fn_name).and_then(|s| first_param_ann(s)),
        _ => None,
    };
    match acc {
        Some(u) => simple_ann(&u),
        None => seed.and_then(|u| simple_ann(&u)),
    }
    .filter(|u| u != "any")
}

/// `__fn(P|..)->(R)` → `P`, the first parameter's annotation.
fn first_param_ann(ann: &str) -> Option<String> {
    let rest = ann.strip_prefix("__fn(")?;
    let close = crate::type_ann_fnsig::close_paren(rest)?;
    let params = &rest[..close];
    let mut depth = 0i32;
    let mut end = params.len();
    let mut prev = '\0';
    for (i, c) in params.char_indices() {
        match c {
            '(' | '<' => depth += 1,
            // the `>` of a nested fn type's `->` closes nothing
            '>' if prev == '-' => {}
            ')' | '>' => depth -= 1,
            '|' if depth == 0 => {
                end = i;
                break;
            }
            _ => {}
        }
        prev = c;
    }
    let first = params[..end].trim();
    (!first.is_empty() && first != "any").then(|| first.to_string())
}

/// The callback's return annotation, when the arg is a lifted closure
/// (`fn_sigs` holds the full `__fn(P|..)->(R)` under its reserved
/// name), a user fn ident (`fn_sigs` holds the bare return ann), or a
/// binding whose ann spells `__fn(..)->(R)`.
pub(super) fn callback_ret_ann(
    exprs: &[Expr],
    arg: ExprId,
    params: &[Param],
    binds: &std::collections::HashMap<String, String>,
    fn_sigs: &std::collections::HashMap<String, String>,
) -> Option<String> {
    match exprs.get(arg.0 as usize)? {
        Expr::Closure { fn_name, .. } => fn_ann_ret(fn_sigs.get(fn_name)?),
        Expr::Ident(n) => {
            if let Some(bare) = fn_sigs.get(n) {
                return simple_ann(bare);
            }
            let ann = params
                .iter()
                .find(|p| &p.name == n)
                .and_then(|p| p.type_ann.clone())
                .or_else(|| binds.get(n).cloned())?;
            fn_ann_ret(&ann)
        }
        _ => None,
    }
}

/// `__fn(P|..)->(R)` → `R`, for a simple `R` only: a composite `R` has
/// no `R[]` spelling this sniff can safely mint.
fn fn_ann_ret(ann: &str) -> Option<String> {
    let rest = ann.strip_prefix("__fn(")?;
    let close = crate::type_ann_fnsig::close_paren(rest)?;
    simple_ann(crate::type_ann_fnsig::ret_of_tail(&rest[close + 1..])?)
}

fn simple_ann(ret: &str) -> Option<String> {
    if ret.is_empty() || ret.contains('(') || ret.contains("->") || ret == "void" {
        return None;
    }
    Some(ret.to_string())
}

#[cfg(test)]
mod tests {
    use super::{Expr, ExprId};
    use std::collections::HashMap;

    /// `reduce(cb, seed)` where the callback's accumulator parameter is
    /// annotated `acc` (or not), against a seed typed `seed`.
    fn reduce(acc: Option<&str>, seed: Option<&str>) -> Option<String> {
        let cb = Expr::Closure {
            fn_name: "__cb".into(),
            captures: Vec::new(),
        };
        let exprs = [cb, Expr::Number(0.0)];
        let mut sigs = HashMap::new();
        if let Some(a) = acc {
            sigs.insert("__cb".to_string(), format!("__fn({a}|number)->(any)"));
        }
        super::reduce_result_ann(
            &exprs,
            &[ExprId(0), ExprId(1)],
            seed.map(str::to_string),
            &sigs,
        )
    }

    #[test]
    fn an_annotated_accumulator_decides_the_result() {
        assert_eq!(
            reduce(Some("string"), Some("number")).as_deref(),
            Some("string")
        );
        assert_eq!(reduce(None, Some("string")).as_deref(), Some("string"));
        // a union the sniff cannot spell is still the accumulator's
        // type: the seed's narrower one is not the answer
        assert_eq!(reduce(Some("__union(number|string)"), Some("string")), None);
        assert_eq!(reduce(None, None), None);
    }
}
