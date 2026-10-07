//! Which bindings a declaration writes that it does not declare itself.

use std::collections::HashSet;

use super::free_vars::{Bound, FreeSink, walk_stmt};
use super::{Ast, Expr, ExprId, Stmt};

/// The names `decl` assigns (or `++` / `--`) that no frame inside it
/// binds: captures, and top-level or global bindings.
pub(crate) fn outer_writes(ast: &Ast, decl: &Stmt) -> Vec<String> {
    struct Sites(Vec<(String, ExprId)>);
    impl FreeSink for Sites {
        fn free(&mut self, name: &str, site: Option<ExprId>, _bound: &[String]) {
            if let Some(site) = site {
                self.0.push((name.to_string(), site));
            }
        }
    }
    let written: HashSet<ExprId> = ast
        .exprs
        .iter()
        .filter_map(|e| match e {
            Expr::Assign { target, .. } | Expr::PostIncr { target, .. } => Some(*target),
            _ => None,
        })
        .collect();
    let mut bound = Bound {
        names: Vec::new(),
        rebound_globals: &[],
    };
    let mut sites = Sites(Vec::new());
    walk_stmt(ast, decl, &mut bound, &mut sites);
    sites
        .0
        .into_iter()
        .filter(|(_, s)| written.contains(s))
        .map(|(n, _)| n)
        .collect()
}
