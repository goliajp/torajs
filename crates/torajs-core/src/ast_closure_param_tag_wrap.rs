//! The named-fn wrap of [`crate::ast_closure_param_tag`]: once the
//! marking fixpoint has settled, the named functions reaching a
//! retagged slot or return lane become their forwarder cells.

use crate::ast::{Ast, Expr, ExprId, Param, Stmt};
use crate::ast_closure_param_tag_member::passthrough_leaves;
use std::collections::{HashMap, HashSet};

/// Named-fn values reaching a retagged lane need the env-first shape
/// too: `f(g)` args at marked params and `return g` sites in
/// ret-marked fns flip to `__forward_<g>` closure values, synthesizing
/// the forwarder decl (same shape as synthesize_fn_to_closure_forwarders).
/// A closure-holding name shadowing a global fn stays unwrapped —
/// the value already carries the env-first shape. (Chunk 774
/// extraction from [`crate::ast_closure_param_tag::tag_closure_arg_params`]'s
/// tail.)
// separate per-fn tables built by the caller pass
#[allow(clippy::too_many_arguments)]
pub(crate) fn wrap_named_fn_values(
    ast: &mut Ast,
    fn_params: &HashMap<String, Vec<(usize, String)>>,
    fn_returns: &HashMap<String, Vec<ExprId>>,
    fn_sigs: &HashMap<String, (Vec<Param>, Option<String>, crate::lexer::Span)>,
    marked: &HashSet<(String, usize)>,
    ret_marked: &HashSet<String>,
    (closure_idents, bound): crate::ast_closure_param_tag::Held,
    alias_init_sites: &[(ExprId, String)],
    existing_forwarders: &mut HashSet<String>,
) {
    // Alias-let inits elected by the main pass's fixpoint (`let
    // alias = h; f(alias)` into a marked slot) — wrap the INIT so
    // the binding's slot re-reprs Closure.
    let mut wrap_sites: Vec<(ExprId, String)> = alias_init_sites.to_vec();
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
            // a choice among named functions (`c ? f1 : f2`) wraps each
            let leaves = args
                .get(*idx)
                .into_iter()
                .flat_map(|a| passthrough_leaves(ast, *a));
            for arg in leaves {
                if let Expr::Ident(g) = ast.get_expr(arg)
                    && fn_sigs.contains_key(g)
                    && !closure_idents.contains(g)
                    && !bound.contains(&arg)
                {
                    wrap_sites.push((arg, g.clone()));
                }
            }
        }
    }
    // name order, not hash order: the synthesized decls land in this order
    let mut returns: Vec<_> = fn_returns.iter().collect();
    returns.sort_by(|a, b| a.0.cmp(b.0));
    for (fname, rets) in returns {
        if !ret_marked.contains(fname) {
            continue;
        }
        for r in rets.iter().flat_map(|r| passthrough_leaves(ast, *r)) {
            if let Expr::Ident(g) = ast.get_expr(r)
                && fn_sigs.contains_key(g)
                && !closure_idents.contains(g)
                && !bound.contains(&r)
            {
                wrap_sites.push((r, g.clone()));
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
        // A `this`-reading declaration already wears the
        // receiver-first face; minting a plain one beside it is the
        // two-cell split RFC 20260914 blade 2 removed.
        let existing = crate::ast::shell_names(target)
            .into_iter()
            .find(|n| existing_forwarders.contains(n));
        let forward_name = existing
            .clone()
            .unwrap_or_else(|| format!("__forward_{target}"));
        if existing.is_none() {
            let (mut params, mut return_type, target_span) = fn_sigs.get(target).unwrap().clone();
            // the snapshot predates the retag: the forwarder hands its
            // params straight to the target, so it takes the target's
            // closure repr wherever the target does
            for (i, p) in params.iter_mut().enumerate() {
                if marked.contains(&(target.clone(), i))
                    && let Some(ann) = &mut p.type_ann
                {
                    crate::ast_closure_param_tag_retag::retag_to_cls(ann);
                }
            }
            if ret_marked.contains(target)
                && let Some(ann) = &mut return_type
            {
                crate::ast_closure_param_tag_retag::retag_to_cls(ann);
            }
            // Knife 4d (RFC 20260801-arguments-method-face) — an
            // argv-consuming generator factory takes its argv from the
            // relay's own `[...arguments]` rather than a declared slot:
            // the relay then carries an arguments touch, the argv/static
            // faces adopt IT, and the true call-site argv reaches the
            // factory however the relay is invoked. That, the hidden
            // `__this` skip and the rest-param spread all live in
            // `ast/forwarder_synth.rs` now — this site used to forward a
            // rest param as a bare ident, the one place the four copies
            // of the rule had drifted apart.
            new_decls.push(crate::ast::synth_forwarder_decl(
                ast,
                &forward_name,
                target,
                &params,
                return_type,
                target_span,
                crate::ast::ShellFace::Plain,
            ));
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
