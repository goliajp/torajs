//! Which bindings a declaration writes that it does not declare itself.

use std::collections::HashMap;

use super::desugar_with::walk::{self, expr_children, stmt_children_ref, stmt_exprs};
use super::free_vars::{Bound, FreeSink, walk_stmt};
use super::{Ast, Expr, ExprId, Stmt};

/// The names `decl` assigns (or `++` / `--`) that no frame inside it
/// binds: captures, and top-level or global bindings.
pub(crate) fn outer_writes(ast: &Ast, decl: &Stmt) -> Vec<String> {
    let free = free_sites(ast, decl);
    ast.exprs
        .iter()
        .filter_map(|e| match e {
            Expr::Assign { target, .. } | Expr::PostIncr { target, .. } => {
                free.get(target).cloned()
            }
            _ => None,
        })
        .collect()
}

/// True when `decl` assigns a name no frame inside it binds a value
/// that reads one it does bind: a parameter or a local of its own.
pub(crate) fn stores_own_value_outside(ast: &Ast, decl: &Stmt) -> bool {
    let free = free_sites(ast, decl);
    ast.exprs.iter().any(|e| {
        matches!(e, Expr::Assign { target, value }
            if free.contains_key(target) && reads_bound(ast, *value, &free))
    })
}

fn reads_bound(ast: &Ast, e: ExprId, free: &HashMap<ExprId, String>) -> bool {
    (matches!(ast.get_expr(e), Expr::Ident(_)) && !free.contains_key(&e))
        || walk::expr_children(ast, e)
            .into_iter()
            .any(|c| reads_bound(ast, c, free))
}

/// The read and write sites in `decl` of names no frame inside it
/// binds, with the name each one spells.
fn free_sites(ast: &Ast, decl: &Stmt) -> HashMap<ExprId, String> {
    struct Sites(HashMap<ExprId, String>);
    impl FreeSink for Sites {
        fn free(&mut self, name: &str, site: Option<ExprId>, _bound: &[String]) {
            if let Some(site) = site {
                self.0.insert(site, name.to_string());
            }
        }
    }
    let mut bound = Bound {
        names: Vec::new(),
        rebound_globals: &[],
    };
    let mut sites = Sites(HashMap::new());
    walk_stmt(ast, decl, &mut bound, &mut sites);
    sites.0
}

/// Does `s` assign `name` (`=`, a compound assignment, `++` / `--`),
/// nested function bodies included? Shadowing is not consulted, so a
/// caller must read a false yes as the safe answer.
pub(crate) fn assigns_name(ast: &Ast, s: &Stmt, name: &str) -> bool {
    if let Stmt::FnDecl { body, .. } = s {
        return body.iter().any(|c| assigns_name(ast, c, name));
    }
    stmt_exprs(s)
        .into_iter()
        .any(|e| expr_assigns(ast, e, name))
        || stmt_children_ref(s)
            .into_iter()
            .any(|c| assigns_name(ast, c, name))
}

fn expr_assigns(ast: &Ast, e: ExprId, name: &str) -> bool {
    let target = match ast.get_expr(e) {
        Expr::Assign { target, .. } | Expr::PostIncr { target, .. } => Some(*target),
        _ => None,
    };
    if target.is_some_and(|t| matches!(ast.get_expr(t), Expr::Ident(n) if n == name)) {
        return true;
    }
    match ast.get_expr(e) {
        Expr::ArrowFn { body, .. } => body.iter().any(|s| assigns_name(ast, s, name)),
        _ => expr_children(ast, e)
            .into_iter()
            .any(|c| expr_assigns(ast, c, name)),
    }
}

/// The sites in `parts` that reference `name` with no frame inside
/// them binding it, or `None` when a lifted closure among them captures
/// `name`: that closure's body lies elsewhere in the arena and may
/// read or write the binding out of the caller's sight.
pub(crate) fn unbound_sites(ast: &Ast, parts: &[&Stmt], name: &str) -> Option<Vec<ExprId>> {
    struct Sites<'a> {
        name: &'a str,
        out: Vec<ExprId>,
        captured: bool,
    }
    impl FreeSink for Sites<'_> {
        fn free(&mut self, name: &str, site: Option<ExprId>, _bound: &[String]) {
            if name != self.name {
                return;
            }
            match site {
                Some(site) => self.out.push(site),
                None => self.captured = true,
            }
        }
    }
    let rebound = [name.to_string()];
    let mut sites = Sites {
        name,
        out: Vec::new(),
        captured: false,
    };
    for s in parts {
        let mut bound = Bound {
            names: Vec::new(),
            rebound_globals: &rebound,
        };
        walk_stmt(ast, s, &mut bound, &mut sites);
    }
    (!sites.captured).then_some(sites.out)
}
