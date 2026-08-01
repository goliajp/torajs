//! RFC 20260801-arguments-method-face — fn-value alias
//! devirtualization, a pre-pass of
//! [`super::arguments_object::desugar_arguments_object`].
//!
//! A declare-then-assign fn-NAME value (`var ref; ref = <genexpr /
//! named fn>`) folds to `let ref = Closure __forward_<f>` (uninit-let
//! splice + the forwarder rename). Calls through the alias then ride
//! the forwarder's declared-arity relay, which drops every over-arity
//! arg — so a generator factory's `[...arguments]` capture reaches
//! the body empty, and the static-argv face never admits `f` (the
//! alias's value read is a non-call reference). When the alias is
//! consumed EXCLUSIVELY by direct calls, rewrite each call's callee
//! to `f` itself and drop the alias decl: the sites become ordinary
//! direct calls, the face analyses see them uniformly, and the relay
//! (plus its arg drop) is bypassed entirely (test262
//! gen-func-expr-args-trailing-comma family).
//!
//! Gated on `f`'s body touching `arguments`: a fn with no arguments
//! face keeps its alias untouched — an over-arity direct call would
//! otherwise turn today's legal pad/drop closure call into a checker
//! arity error.

use super::arguments_object_walkers::{
    body_has_arguments_length, body_has_non_length_arguments_touch,
};
use super::{Ast, Expr, Stmt};

pub(super) fn devirtualize_fn_value_aliases(ast: &mut Ast) {
    use std::collections::{HashMap, HashSet};
    // alias binding name → forwarder target fn name, for top-level
    // `let x = Closure __forward_<f> captures=[]` decls whose target
    // exists as a top-level FnDecl with an arguments touch.
    let mut aliases: HashMap<String, (usize, String)> = HashMap::new();
    for (si, s) in ast.stmts.iter().enumerate() {
        let Stmt::LetDecl { name, init, .. } = s else {
            continue;
        };
        let Expr::Closure { fn_name, captures } = ast.get_expr(*init) else {
            continue;
        };
        if !captures.is_empty() {
            continue;
        }
        let Some(target) = fn_name.strip_prefix("__forward_") else {
            continue;
        };
        let touches = ast.stmts.iter().any(|s| {
            matches!(s, Stmt::FnDecl { name: n, body, .. }
                if n == target
                    && (body_has_arguments_length(ast, body)
                        || body_has_non_length_arguments_touch(ast, body)))
        });
        if touches {
            aliases.insert(name.clone(), (si, target.to_string()));
        }
    }
    if aliases.is_empty() {
        return;
    }
    // An alias survives only when every arena reference is a direct
    // Call callee or its own decl init slot — same conservatism as
    // the binding-safety walk (any other use kills it: those sites
    // still need the closure value).
    let mut drop_idxs: Vec<usize> = Vec::new();
    for (name, (si, target)) in &aliases {
        let eids: HashSet<u32> = (0..ast.exprs.len() as u32)
            .filter(|&i| matches!(&ast.exprs[i as usize], Expr::Ident(n) if n == name))
            .collect();
        let mut call_sites: Vec<u32> = Vec::new();
        for e in &ast.exprs {
            if let Expr::Call { callee, .. } = e
                && eids.contains(&callee.0)
            {
                call_sites.push(callee.0);
            }
        }
        if eids.len() != call_sites.len() || call_sites.is_empty() {
            continue;
        }
        for eid in call_sites {
            ast.exprs[eid as usize] = Expr::Ident(target.clone());
        }
        drop_idxs.push(*si);
    }
    drop_idxs.sort_unstable_by(|a, b| b.cmp(a));
    for si in drop_idxs {
        ast.stmts.remove(si);
    }
}
