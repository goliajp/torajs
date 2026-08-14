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

use super::{Expr, Param};

/// The callback's return annotation, when the arg is a lifted closure
/// (`fn_sigs` holds the full `__fn(P|..)->R` under its reserved
/// name), a user fn ident (`fn_sigs` holds the bare return ann), or a
/// binding whose ann spells `__fn(..)->R`.
pub(super) fn callback_ret_ann(
    exprs: &[Expr],
    arg: super::ExprId,
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

/// `__fn(P|..)->R` → `R`, for a simple `R` only — a nested arrow in
/// the return position would make the tail split ambiguous, and a
/// composite `R` has no `R[]` spelling this sniff can safely mint.
fn fn_ann_ret(ann: &str) -> Option<String> {
    let rest = ann.strip_prefix("__fn(")?;
    let (_, ret) = rest.rsplit_once("->")?;
    simple_ann(ret)
}

fn simple_ann(ret: &str) -> Option<String> {
    if ret.is_empty() || ret.contains('(') || ret.contains("->") || ret == "void" {
        return None;
    }
    Some(ret.to_string())
}
