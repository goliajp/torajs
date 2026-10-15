//! Which bindings of a body hold only an unclaimed function
//! expression: the names [`super::misroutes`] follows a value through.

use std::collections::{HashMap, HashSet};

use super::{Held, outer_of};
use crate::ast::desugar_with::walk::stmt_children_ref;
use crate::ast::fnexpr_this_default::unclaimed_fnexpr;
use crate::ast::fnexpr_this_names::peel_as;
use crate::ast::{Ast, Expr, ExprId, Param, Stmt};

/// Every name some `x = …` anywhere in the program writes.
pub(super) fn assigned_names(ast: &Ast) -> HashSet<&str> {
    ast.exprs
        .iter()
        .filter_map(|e| match e {
            Expr::Assign { target, .. } => match ast.get_expr(*target) {
                Expr::Ident(n) => Some(n.as_str()),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

/// Bindings visible in this body whose only value is an unclaimed
/// function expression, each with the receiver kind of the body that
/// minted it: its own declarations, and the ones a lifted arrow
/// captures from the body that minted the arrow and does not
/// redeclare.
pub(super) fn held_of<'a>(
    ast: &'a Ast,
    fns: &HashMap<&'a str, (&'a [Param], &'a [Stmt])>,
    mints: &HashMap<&'a str, (ExprId, &'a str)>,
    assigned: &HashSet<&str>,
    name: &str,
    depth: usize,
) -> HashMap<&'a str, Held> {
    let Some(&(params, body)) = fns.get(name) else {
        return HashMap::new();
    };
    let mut decls: Vec<(&'a str, ExprId)> = Vec::new();
    collect_decls(body, &mut decls);
    let mut held: HashMap<&'a str, Held> = HashMap::new();
    let caps = params
        .first()
        .filter(|p| p.name == "__env")
        .and_then(|p| p.type_ann.as_deref())
        .and_then(|a| a.strip_prefix("__env(")?.strip_suffix(')'));
    if let (Some(caps), Some(&(_, minter)), true) = (caps, mints.get(name), depth < 64) {
        let outer = held_of(ast, fns, mints, assigned, minter, depth + 1);
        let own_this = params.iter().any(|p| p.name == "__this")
            || body.iter().any(super::super::stmt_declares_this);
        for c in caps.split('|') {
            let shadowed = decls.iter().any(|(n, _)| *n == c) || params.iter().any(|p| p.name == c);
            if let Some((&n, &(k, same))) = outer.get_key_value(c).filter(|_| !shadowed) {
                held.insert(n, (k, same && !own_this));
            }
        }
    }
    let own = outer_of(ast, fns, mints, name, 0);
    let mut count: HashMap<&str, usize> = HashMap::new();
    for (n, _) in &decls {
        *count.entry(n).or_default() += 1;
    }
    let single = |n: &str| count.get(n) == Some(&1) && !assigned.contains(n);
    loop {
        let before = held.len();
        for &(n, init) in &decls {
            if held.contains_key(n) || !single(n) {
                continue;
            }
            let v = peel_as(&ast.exprs, init);
            let kind = match ast.get_expr(v) {
                Expr::Ident(src) => held.get(src.as_str()).copied(),
                _ => own
                    .filter(|_| unclaimed_fnexpr(ast, v).is_some())
                    .map(|k| (k, true)),
            };
            if let Some(k) = kind {
                held.insert(n, k);
            }
        }
        if held.len() == before {
            return held;
        }
    }
}

/// Declarations in this body, nested blocks included, nested functions
/// excluded.
pub(super) fn collect_decls<'a>(stmts: &'a [Stmt], out: &mut Vec<(&'a str, ExprId)>) {
    for s in stmts {
        match s {
            Stmt::FnDecl { .. } => {}
            Stmt::LetDecl { name, init, .. } => out.push((name, *init)),
            other => {
                for c in stmt_children_ref(other) {
                    collect_decls(std::slice::from_ref(c), out);
                }
            }
        }
    }
}
