//! Does a declaration write a binding it does not declare itself?

use std::collections::HashSet;

use super::free_vars::{Bound, FreeSink, walk_stmt};
use super::{Ast, Expr, ExprId, Stmt};

/// True when `decl` assigns (or `++` / `--`) a name that no frame
/// inside it binds: a capture, or a top-level or global binding.
pub(crate) fn writes_outer_binding(ast: &Ast, decl: &Stmt) -> bool {
    struct Sites(Vec<ExprId>);
    impl FreeSink for Sites {
        fn free(&mut self, _name: &str, site: Option<ExprId>, _bound: &[String]) {
            if let Some(site) = site {
                self.0.push(site);
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
    sites.0.iter().any(|s| written.contains(s))
}
