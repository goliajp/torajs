//! HOF-elem axis (551-01) — a builtin array higher-order method's
//! callback receives the receiver's ELEMENTS in its element-position
//! parameter(s). A fn-typed array's elements are Closure repr across
//! the board (chunk 733 — the fact `is_closure_shaped`'s Index arm
//! encodes), so a fn-typed (`__fn(`-annotated) element-position
//! parameter must carry the env-first shape. Left bare, the lowered
//! HOF loop hands each closure cell to a bare-FnSig slot and the
//! callback's call `blr`s the cell — the bug-327 SIGBUS family
//! through a lane no AST Call edge reaches: the element→param
//! hand-off happens inside the lowered loop, so the parent pass's
//! call-site rounds never see it (`fs.map((f) => f())` was a silent
//! zero-output death).
//!
//! Same approximation grade as the rest of the pass: no receiver
//! typing — any `.map(cb)` whose callback has a fn-typed
//! element-position param seeds. Over-marking costs that one param
//! its direct-dispatch ABI; a fn value is a fn value either way and
//! the forwarder wrap keeps its call sites sound. A callback that is
//! itself a fn-typed param name (`function h(cb) { fs.map(cb) }`)
//! stays unseeded — plan-state L3b residue with the parent pass's
//! other per-name misses.

use crate::ast::{Ast, Expr, Stmt};
use crate::ast_closure_param_tag::push_child_stmts;
use std::collections::{HashMap, HashSet};

/// Element-position USER-param indices of a builtin HOF's callback
/// (before any lifted `__env` offset).
fn hof_elem_positions(name: &str) -> Option<&'static [usize]> {
    match name {
        "map" | "forEach" | "filter" | "find" | "findLast" | "findIndex" | "findLastIndex"
        | "some" | "every" | "flatMap" => Some(&[0]),
        "sort" | "toSorted" => Some(&[0, 1]),
        "reduce" | "reduceRight" => Some(&[1]),
        _ => None,
    }
}

/// Seed marks for fn-typed element-position params of callbacks
/// handed to builtin HOF member calls anywhere in the program.
pub(crate) fn hof_elem_cb_param_seeds(
    ast: &Ast,
    fn_params: &HashMap<String, Vec<(usize, String)>>,
) -> HashSet<(String, usize)> {
    // FnDecl name → param offset (a lifted closure's first param is
    // its `__env` pointer; user params start behind it).
    let mut env_offset: HashMap<&str, usize> = HashMap::new();
    let mut stack: Vec<&Stmt> = ast.stmts.iter().collect();
    while let Some(s) = stack.pop() {
        if let Stmt::FnDecl { name, params, .. } = s {
            let off = usize::from(params.first().is_some_and(|p| p.name == "__env"));
            env_offset.insert(name.as_str(), off);
        }
        push_child_stmts(s, &mut stack);
    }
    let mut seeds = HashSet::new();
    for e in &ast.exprs {
        let Expr::Call { callee, args } = e else {
            continue;
        };
        let Expr::Member { name, .. } = ast.get_expr(*callee) else {
            continue;
        };
        let Some(positions) = hof_elem_positions(name) else {
            continue;
        };
        let Some(&cb) = args.first() else {
            continue;
        };
        let cb_fn = match ast.get_expr(cb) {
            Expr::Closure { fn_name, .. } => fn_name.as_str(),
            Expr::Ident(n) => n.as_str(),
            _ => continue,
        };
        let Some(fps) = fn_params.get(cb_fn) else {
            continue;
        };
        let off = env_offset.get(cb_fn).copied().unwrap_or(0);
        for pos in positions {
            let idx = off + pos;
            if fps.iter().any(|(i, _)| *i == idx) {
                seeds.insert((cb_fn.to_string(), idx));
            }
        }
    }
    seeds
}
