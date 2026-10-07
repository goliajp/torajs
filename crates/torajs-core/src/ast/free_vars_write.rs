//! Which bindings a declaration writes that it does not declare itself.

use std::collections::HashMap;

use super::desugar_with::walk;
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
