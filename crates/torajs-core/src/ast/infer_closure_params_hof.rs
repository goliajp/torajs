//! Array HOF param-only seed arms for
//! [`super::infer_closure_params`] — moved verbatim from
//! `infer_closure_params_helpers.rs` when the cluster-#1 blade-2
//! position work (spec §23.1.3 `(elem, index, srcArray)` seeds)
//! pushed that file past the 500-line cap.

use std::collections::HashMap;

use super::infer_closure_params::infer_lit_ann;
use super::{Ast, Expr, ExprId};

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
    view_promotes: &mut HashMap<String, (usize, String)>,
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
                param_only_updates.insert(fn_name.clone(), hof_pos_anns(elem_ann, p));
            }
            view_promotes.insert(fn_name.clone(), (2, elem_ann.to_string()));
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
                param_only_updates.insert(fn_name.clone(), hof_pos_anns(elem_ann, p));
            }
            view_promotes.insert(fn_name.clone(), (2, elem_ann.to_string()));
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
            // (acc, cur, index, srcArray) per §23.1.3.24.
            let mut param_anns = vec![acc_ann.clone()];
            param_anns.extend(hof_pos_anns(elem_ann, p.saturating_sub(1)));
            param_anns.truncate(p);
            param_only_updates.insert(fn_name.clone(), param_anns);
            view_promotes.insert(fn_name.clone(), (3, elem_ann.to_string()));
        }
        return true;
    }
    false
}

/// Spec §23.1.3 callback position annotations — `(elem, index,
/// srcArray)`, truncated to the callback's declared user-param count.
/// The srcArray slot is `any[]`, not `{elem}[]`: a numeric literal
/// array often lives in an i64-slot block, and only the Arr<Any> view
/// reads kind-aware (the chunk 625/626 protocol — the call boundary
/// marks the block's elem kind, `a[i]` reboxes per the record). A
/// `{elem}[]` view would raw-LoadDyn f64 over i64 slots.
fn hof_pos_anns(elem_ann: &str, p: usize) -> Vec<String> {
    let mut v = vec![
        elem_ann.to_string(),
        "number".to_string(),
        "any[]".to_string(),
    ];
    v.truncate(p);
    v
}
