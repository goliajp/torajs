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

//! chunk 632 — backward round: `f(cb2)` where `(f, idx)` is already
//! marked (env-first ABI) and `cb2` names another fn's `__fn(`
//! param. The passing site would hand a bare FnSig value to a slot
//! the callee reads as a closure env block — a pre-existing silent
//! crash (probe p631c, rc 138) on the closure axis, newly reachable
//! through the replace-cb axis too. Marking the source param makes
//! its own call sites wrap, so the closure shape propagates against
//! the call direction until the chain bottoms out at real values.

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

/// One backward-marking round (see module doc): fn-typed params
/// whose name is passed as an argument into an already-marked param
/// slot. Returns the marks to add; the caller's fixpoint reruns this
/// until the chain closes. Per-name attribution, same approximation
/// as the seed round.
pub(crate) fn backward_param_marks(
    ast: &Ast,
    fn_params: &HashMap<String, Vec<(usize, String)>>,
    marked: &HashSet<(String, usize)>,
) -> HashSet<(String, usize)> {
    let mut param_owners: HashMap<&str, Vec<(&str, usize)>> = HashMap::new();
    for (fname, fps) in fn_params {
        for (idx, pname) in fps {
            param_owners
                .entry(pname.as_str())
                .or_default()
                .push((fname.as_str(), *idx));
        }
    }
    let mut adds = HashSet::new();
    for e in &ast.exprs {
        let Expr::Call { callee, args } = e else {
            continue;
        };
        let Expr::Ident(f) = ast.get_expr(*callee) else {
            continue;
        };
        for (fname, idx) in marked {
            if fname != f {
                continue;
            }
            if let Some(arg) = args.get(*idx)
                && let Expr::Ident(n) = ast.get_expr(*arg)
            {
                for (owner, oidx) in param_owners.get(n.as_str()).into_iter().flatten() {
                    let key = (owner.to_string(), *oidx);
                    if !marked.contains(&key) {
                        adds.insert(key);
                    }
                }
            }
        }
    }
    adds
}
