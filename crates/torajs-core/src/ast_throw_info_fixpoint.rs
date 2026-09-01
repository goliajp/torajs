//! The transitive may-throw fixed point over the program's FnDecls —
//! moved out of [`crate::ast_throw_info`] as its own sibling (rotation
//! 550, pure motion): that file answers one fn's throw shape
//! (`fn_throw_info` and the stmt / expr scans), this one closes the
//! per-fn answers over the call graph.

use crate::ast::{Ast, ExprId, Stmt};
use crate::ast_throw_info::fn_throw_info;
use std::collections::{HashMap, HashSet};

/// M4.3.b — the transitive may-throw set: collect each FnDecl's
/// `(direct_throw, called_names)` then iterate to fixed-point — a fn
/// is may_throw if it throws directly OR calls any may_throw fn.
pub fn compute_may_throw_fns(
    ast: &Ast,
    expr_types: &HashMap<ExprId, crate::check::Type>,
) -> HashSet<String> {
    let mut may_throw: HashSet<String> = HashSet::new();
    let mut decl_throw_info: Vec<(String, bool, Vec<String>)> = Vec::new();
    // Rotation 550 (550-01) — a call through a let-bound arrow
    // (`const boom = () => { throw … }; boom()`) records the BINDING
    // name, but the body lives in the lifted `__closure_N` FnDecl;
    // resolving through the same alias table the param-tag / infer
    // rounds use (549-02) lets the fixed point below reach it.
    // Without it every callback whose only throw source was such a
    // call was judged never-throwing, the HOF loop pruned its check,
    // and the pending throw strayed into the NEXT checked call
    // (`for … try { a(i).map(x => x + boom()) } catch` caught once
    // in three).
    let aliases = crate::ast_closure_param_tag_collect::closure_let_aliases(ast);
    for stmt in &ast.stmts {
        if let Stmt::FnDecl {
            name, params, body, ..
        } = stmt
        {
            let (direct, mut called) = fn_throw_info(ast, params, body, expr_types);
            if direct {
                may_throw.insert(name.clone());
            }
            let lifted: Vec<String> = called
                .iter()
                .filter_map(|c| aliases.get(c).map(|(f, _)| f.clone()))
                .collect();
            called.extend(lifted);
            // Rotation 507 — a `__dispatch_<M>` stub's AST body only
            // forwards to the base owner, but the slot it stands for
            // resolves to EVERY owner's body at runtime: a throwing
            // override behind a base-typed call was invisible here, so
            // the caller of a fn that only called the stub never
            // checked, printed the ret sentinel 0 and ran on (probe:
            // `viaParam(new Other(13))` — the uncaught error surfaced
            // at exit instead of in the enclosing try).
            if let Some(m) = name.strip_prefix("__dispatch_") {
                let (bare, suffix) = m
                    .split_once("$$")
                    .map(|(b, s)| (b, format!("$${s}")))
                    .unwrap_or((m, String::new()));
                for o in ast.method_owners.get(bare).into_iter().flatten() {
                    called.push(format!("__cm_{o}__{bare}"));
                    called.push(format!("__cm_{o}__{bare}{suffix}"));
                }
            }
            decl_throw_info.push((name.clone(), direct, called));
        }
    }
    loop {
        let mut grew = false;
        for (name, _direct, called) in &decl_throw_info {
            if may_throw.contains(name) {
                continue;
            }
            for c in called {
                if may_throw.contains(c) {
                    may_throw.insert(name.clone());
                    grew = true;
                    break;
                }
            }
        }
        if !grew {
            break;
        }
    }
    may_throw
}
