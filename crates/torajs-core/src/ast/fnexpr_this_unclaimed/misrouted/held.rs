//! Which bindings of a body hold only an unclaimed function
//! expression: the names [`super::misroutes`] and [`super::escape`]
//! follow a value through.

use std::collections::{HashMap, HashSet};

use super::{Held, outer_of};
use crate::ast::desugar_with::walk::stmt_children_ref;
use crate::ast::fnexpr_this_default::unclaimed_fnexpr;
use crate::ast::fnexpr_this_names::peel_as;
use crate::ast::{Ast, Expr, ExprId, Param, Stmt};

/// Every name some `x = …` anywhere in the program writes, with the
/// values written.
pub(super) fn assigned_names(ast: &Ast) -> HashMap<&str, Vec<ExprId>> {
    let mut out: HashMap<&str, Vec<ExprId>> = HashMap::new();
    for e in &ast.exprs {
        if let Expr::Assign { target, value } = e
            && let Expr::Ident(n) = ast.get_expr(*target)
        {
            out.entry(n.as_str()).or_default().push(*value);
        }
    }
    out
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
    assigned: &HashMap<&str, Vec<ExprId>>,
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
    let single = |n: &str| count.get(n) == Some(&1) && !assigned.contains_key(n);
    // what a value written into a binding holds: such an expression, or
    // a binding already known to hold one
    let value_kind = |held: &HashMap<&'a str, Held>, v: ExprId| {
        let v = peel_as(&ast.exprs, v);
        match ast.get_expr(v) {
            Expr::Ident(src) => held.get(src.as_str()).copied(),
            _ => own
                .filter(|_| unclaimed_fnexpr(ast, v).is_some())
                .map(|k| (k, true)),
        }
    };
    // a name declared without a value (a hoisted `var` is, see
    // `var_hoist`) whose every write anywhere holds the same such value
    let stored_only = |held: &HashMap<&'a str, Held>, n: &str, init: ExprId| {
        if count.get(n) != Some(&1) || !matches!(ast.get_expr(init), Expr::Uninit) {
            return None;
        }
        let mut kinds = assigned.get(n)?.iter().map(|&v| value_kind(held, v));
        let first = kinds.next()??;
        kinds.all(|k| k == Some(first)).then_some(first)
    };
    loop {
        let before = held.len();
        for &(n, init) in &decls {
            if held.contains_key(n) {
                continue;
            }
            let kind = if single(n) {
                value_kind(&held, init)
            } else {
                stored_only(&held, n, init)
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

/// Names this body declares more than once, every time by `let` /
/// `const`, and never writes: distinct block-scoped bindings, each
/// holding its initializer alone. The scan cannot tell which of them a
/// use reaches, so it follows none of them.
pub(super) fn scoped_twice<'a>(
    body: &'a [Stmt],
    assigned: &HashMap<&str, Vec<ExprId>>,
) -> HashSet<&'a str> {
    let mut decls: Vec<(&'a str, bool)> = Vec::new();
    let mut todo: Vec<&'a Stmt> = body.iter().collect();
    while let Some(s) = todo.pop() {
        match s {
            Stmt::FnDecl { .. } => {}
            Stmt::LetDecl { name, is_var, .. } => decls.push((name, *is_var)),
            other => todo.extend(stmt_children_ref(other)),
        }
    }
    let mut out = HashSet::new();
    for &(n, _) in &decls {
        let mut of_n = decls.iter().filter(|(m, _)| *m == n);
        if of_n.clone().count() > 1 && of_n.all(|(_, var)| !var) && !assigned.contains_key(n) {
            out.insert(n);
        }
    }
    out
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
