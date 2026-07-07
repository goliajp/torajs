//! Usage-axis seeds for [`crate::ast_closure_param_tag`] — marking
//! rounds that originate from HOW an `__fn(`-annotated param is
//! consumed, rather than from the value shape reaching it at a call
//! site.
//!
//! chunk 631 — replace-cb axis: `s.replace(pat, cb)` /
//! `s.replaceAll(pat, cb)` where `cb` names a fn-typed param. The
//! functional-replaceValue runtime lane invokes its callback through
//! the closure boxed entry (+32); a bare FnSig has neither env nor
//! boxed entry, so the param must carry the env-first closure shape
//! (chunk 617 residual — the AST forwarder wrap there only covers
//! bare top-FnDecl Ident arguments, not FnSig-typed params).
//!
//! Attribution is per-name program-wide (no scope precision), the
//! same approximation as the parent pass's closure-ident collection:
//! a name collision over-marks a param, costing that one cold param
//! its direct-dispatch ABI; correctness is preserved by the forwarder
//! wrap at its call sites.

use crate::ast::{Ast, Expr};
use std::collections::{HashMap, HashSet};

/// Seed marks for fn-typed params whose name appears as the
/// functional replaceValue of a `replace` / `replaceAll` member call
/// anywhere in the program.
pub(crate) fn replace_cb_param_seeds(
    ast: &Ast,
    fn_params: &HashMap<String, Vec<(usize, String)>>,
) -> HashSet<(String, usize)> {
    let mut cb_names: HashSet<&str> = HashSet::new();
    for e in &ast.exprs {
        if let Expr::Call { callee, args } = e
            && let Expr::Member { name, .. } = ast.get_expr(*callee)
            && matches!(name.as_str(), "replace" | "replaceAll")
            && args.len() >= 2
            && let Expr::Ident(n) = ast.get_expr(args[1])
        {
            cb_names.insert(n);
        }
    }
    let mut seeds = HashSet::new();
    if cb_names.is_empty() {
        return seeds;
    }
    for (fname, fps) in fn_params {
        for (idx, pname) in fps {
            if cb_names.contains(pname.as_str()) {
                seeds.insert((fname.clone(), *idx));
            }
        }
    }
    seeds
}
