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

use crate::ast::{Ast, Expr, ExprId, Param, Stmt};
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

/// Named-fn values reaching a retagged lane need the env-first shape
/// too: `f(g)` args at marked params and `return g` sites in
/// ret-marked fns flip to `__forward_<g>` closure values, synthesizing
/// the forwarder decl (same shape as synthesize_fn_to_closure_forwarders).
/// A closure-holding name shadowing a global fn stays unwrapped —
/// the value already carries the env-first shape. (Chunk 774
/// extraction from [`crate::ast_closure_param_tag::tag_closure_arg_params`]'s
/// tail.)
#[allow(clippy::too_many_arguments)]
pub(crate) fn wrap_named_fn_values(
    ast: &mut Ast,
    fn_params: &HashMap<String, Vec<(usize, String)>>,
    fn_returns: &HashMap<String, Vec<ExprId>>,
    fn_sigs: &HashMap<String, (Vec<Param>, Option<String>, crate::lexer::Span)>,
    marked: &HashSet<(String, usize)>,
    ret_marked: &HashSet<String>,
    closure_idents: &HashSet<String>,
    existing_forwarders: &mut HashSet<String>,
) {
    let mut wrap_sites: Vec<(ExprId, String)> = Vec::new();
    for e in &ast.exprs {
        let Expr::Call { callee, args } = e else {
            continue;
        };
        let Expr::Ident(fname) = ast.get_expr(*callee) else {
            continue;
        };
        for (idx, _) in fn_params.get(fname).into_iter().flatten() {
            if !marked.contains(&(fname.clone(), *idx)) {
                continue;
            }
            if let Some(arg) = args.get(*idx)
                && let Expr::Ident(g) = ast.get_expr(*arg)
                && fn_sigs.contains_key(g)
                && !closure_idents.contains(g)
            {
                wrap_sites.push((*arg, g.clone()));
            }
        }
    }
    for (fname, rets) in fn_returns {
        if !ret_marked.contains(fname) {
            continue;
        }
        for r in rets {
            if let Expr::Ident(g) = ast.get_expr(*r)
                && fn_sigs.contains_key(g)
                && !closure_idents.contains(g)
            {
                wrap_sites.push((*r, g.clone()));
            }
        }
    }
    if wrap_sites.is_empty() {
        return;
    }
    let mut renames: HashMap<String, String> = HashMap::new();
    let mut new_decls: Vec<Stmt> = Vec::new();
    for (_, target) in &wrap_sites {
        if renames.contains_key(target) {
            continue;
        }
        let forward_name = format!("__forward_{target}");
        if !existing_forwarders.contains(&forward_name) {
            let (params, return_type, target_span) = fn_sigs.get(target).unwrap().clone();
            // Knife 4d (RFC 20260801-arguments-method-face) — an
            // argv-consuming generator factory (trailing
            // GEN_ARGV_PARAM, the obj-gen parser's argv channel)
            // takes its argv from the relay's own `[...arguments]`
            // rather than a declared slot: the relay then carries an
            // arguments touch, the argv/static faces adopt IT, and
            // the true call-site argv reaches the factory however
            // the relay is invoked (member call or escaped alias).
            let (declared, takes_gen_argv) = crate::ast::split_gen_argv_tail(&params);
            let declared = declared.to_vec();
            let mut fwd_params: Vec<Param> = Vec::with_capacity(declared.len() + 1);
            fwd_params.push(Param {
                name: "__env".into(),
                type_ann: Some("__env()".to_string()),
                default: None,
                is_rest: false,
            });
            fwd_params.extend(declared.iter().cloned());
            let mut arg_eids: Vec<ExprId> = declared
                .iter()
                .map(|p| ast.add_expr(Expr::Ident(p.name.clone())))
                .collect();
            if takes_gen_argv {
                crate::ast::push_gen_argv_spread(ast, &mut arg_eids);
            }
            let callee_id = ast.add_expr(Expr::Ident(target.clone()));
            let call_id = ast.add_expr(Expr::Call {
                callee: callee_id,
                args: arg_eids,
            });
            new_decls.push(Stmt::FnDecl {
                name: forward_name.clone(),
                type_params: Vec::new(),
                params: fwd_params,
                return_type,
                body: vec![Stmt::Return(Some(call_id))],
                is_generator: false,
                // B4 — carry the TARGET's span (toString answers the
                // wrapped user fn's source).
                span: target_span,
            });
            existing_forwarders.insert(forward_name.clone());
        }
        renames.insert(target.clone(), forward_name);
    }
    for (eid, target) in wrap_sites {
        let forward_name = renames.get(&target).unwrap();
        ast.exprs[eid.0 as usize] = Expr::Closure {
            fn_name: forward_name.clone(),
            captures: Vec::new(),
        };
    }
    ast.stmts.extend(new_decls);
}
