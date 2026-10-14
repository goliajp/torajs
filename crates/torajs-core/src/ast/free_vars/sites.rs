//! The reference sites of one name, reported through the free-variable
//! walk's [`FreeSink`] — split out of the parent under the file-size
//! line; the walk itself stays there.

use super::{Bound, FreeSink, walk_fn, walk_stmt};
use crate::ast::{Ast, Expr, ExprId, Stmt};
use std::collections::HashSet;

/// The sites in one top-level statement where a reference to `name`
/// resolves to the top level — no frame between binds it — and where
/// none of `also_free` is bound either. Two spellings of one top-level
/// binding are interchangeable exactly at such a site. A spelling on
/// the global-name list is still a binding here, so it reports.
pub(crate) fn top_ref_sites(ast: &Ast, s: &Stmt, name: &str, also_free: &[String]) -> Vec<ExprId> {
    struct Sites<'a> {
        name: &'a str,
        also_free: &'a [String],
        out: Vec<ExprId>,
    }
    impl FreeSink for Sites<'_> {
        fn free(&mut self, name: &str, site: Option<ExprId>, bound: &[String]) {
            if name == self.name
                && let Some(site) = site
                && !self.also_free.iter().any(|f| bound.contains(f))
            {
                self.out.push(site);
            }
        }
    }
    let mut rebound_globals: Vec<String> = vec![name.to_string()];
    rebound_globals.extend(also_free.iter().cloned());
    let mut bound = Bound {
        names: Vec::new(),
        rebound_globals: &rebound_globals,
    };
    let mut sites = Sites {
        name,
        also_free,
        out: Vec::new(),
    };
    walk_stmt(ast, s, &mut bound, &mut sites);
    sites.out
}

/// The `Ident` sites in one top-level statement whose free name is
/// one of `names` — no frame between the site and the top level binds
/// it. The module resolver asks this about the names another module
/// declared (`modules::module_scope`); a spelling on the global-name
/// list still reports, as in [`top_ref_sites`].
pub(crate) fn foreign_ref_sites(ast: &Ast, s: &Stmt, names: &HashSet<String>) -> Vec<ExprId> {
    struct Sites<'a> {
        ast: &'a Ast,
        names: &'a HashSet<String>,
        out: Vec<ExprId>,
    }
    impl FreeSink for Sites<'_> {
        fn free(&mut self, name: &str, site: Option<ExprId>, _bound: &[String]) {
            if let Some(site) = site
                && self.names.contains(name)
                && matches!(self.ast.get_expr(site), Expr::Ident(_))
            {
                self.out.push(site);
            }
        }
    }
    let rebound_globals: Vec<String> = names.iter().cloned().collect();
    let mut bound = Bound {
        names: Vec::new(),
        rebound_globals: &rebound_globals,
    };
    let mut sites = Sites {
        ast,
        names,
        out: Vec::new(),
    };
    walk_stmt(ast, s, &mut bound, &mut sites);
    sites.out
}

/// A reference to one of the asked names that no frame inside the
/// walked code binds: it reads the binding around that code.
pub(crate) struct ScopedRef {
    /// The `Ident`, or the `New` carrying the name as its class.
    pub(crate) site: ExprId,
    pub(crate) name: String,
    /// Whether a function between the site and the walked code binds
    /// its own `this` (a function declaration or expression, an
    /// object-literal method, a class member or static initializer).
    pub(crate) own_this: bool,
}

/// The references in `body` to `names` that resolve outside it. `body`
/// is one frame binding `prebound` (a function's parameters, say) and
/// its own declarations; every scope inside it (a block, a declaration
/// list, a loop head, a catch parameter, a nested function or class)
/// keeps the references to a name it declares again. A `"__this"` in
/// `prebound` marks the frame as one that binds its own `this`.
pub(crate) fn scoped_refs(
    ast: &Ast,
    prebound: &[String],
    body: &[Stmt],
    names: &[String],
) -> Vec<ScopedRef> {
    struct Refs<'a> {
        names: &'a [String],
        out: Vec<ScopedRef>,
    }
    impl FreeSink for Refs<'_> {
        fn free(&mut self, name: &str, site: Option<ExprId>, bound: &[String]) {
            if let Some(site) = site
                && self.names.iter().any(|n| n == name)
            {
                self.out.push(ScopedRef {
                    site,
                    name: name.to_string(),
                    own_this: bound.iter().any(|b| b == "__this"),
                });
            }
        }
    }
    let mut bound = Bound {
        names: prebound.to_vec(),
        rebound_globals: names,
    };
    let mut refs = Refs {
        names,
        out: Vec::new(),
    };
    walk_fn(ast, &mut bound, &[], body, &mut refs);
    refs.out
}
