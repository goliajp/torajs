//! A declaration whose only reads of the enclosing scope sit in its
//! parameter defaults.
//!
//! Such a default stays on the call-site pad: a guard in the body would
//! widen its typed slot to `any`, and routing the declaration makes it a
//! closure called indirectly. The pad pastes the default into each call,
//! where its names resolve in the CALLER's scope; §10.2.11 evaluates it
//! in the callee's, which for a nested declaration is the list it is
//! declared in. The two agree when every call is made from that list's
//! own function with no frame between rebinding a name the defaults
//! read — then the declaration keeps its lane and its typed slots
//! (a helper called in a hot loop stays a direct call). Anything else
//! routes:
//!
//! ```text
//! function outer(k: number) {
//!   function f(y: number = k) { return y; }
//!   f();                    // keeps the lane: the pad reads outer's k
//!   [1].map(() => f());     // routes: the pad would sit in a closure
//!   { let k = 9; f(); }     // routes: the pad would read this k
//!   return f;               // routes: a value call pads nothing
//! }
//! ```

use std::collections::HashSet;

use crate::ast::desugar_with::walk::{arrow_nodes, stmt_children_ref};
use crate::ast::free_vars::{Bound, FreeSink, free_vars_of_body, free_vars_of_fn, walk_stmt};
use crate::ast::free_vars_expr::walk_expr;
use crate::ast::new_target_param::NEW_TARGET;
use crate::ast::{Ast, Expr, ExprId, Stmt};

/// See the module doc. `site` indexes the declaration in `stmts`,
/// `prebound` holds its params and the top-level names still in scope.
pub(super) fn pad_reads_scope(
    ast: &Ast,
    stmts: &[Stmt],
    site: usize,
    prebound: &[String],
    peers: &HashSet<&str>,
) -> bool {
    let Stmt::FnDecl {
        name, params, body, ..
    } = &stmts[site]
    else {
        unreachable!("site is a FnDecl");
    };
    if free_vars_of_body(ast, prebound, body)
        .iter()
        .any(|n| !peers.contains(n.as_str()))
    {
        return false;
    }
    let read = free_vars_of_fn(ast, prebound, params, &[]);
    // the function's own `arguments`, `this` and `new.target` are not
    // the caller's
    if read.iter().any(|n| {
        peers.contains(n.as_str()) || matches!(n.as_str(), "arguments" | "__this" | NEW_TARGET)
    }) {
        return false;
    }
    let mut rebound = vec![name.clone()];
    rebound.extend(read.iter().cloned());
    let mut in_closures = Sites::new(name, &read);
    for s in stmts {
        for_each_arrow(ast, s, &mut |a| {
            walk_expr(ast, a, &mut frame(&rebound), &mut in_closures)
        });
    }
    let callees: HashSet<ExprId> = ast
        .exprs
        .iter()
        .filter_map(|e| match e {
            Expr::Call { callee, .. } => Some(*callee),
            _ => None,
        })
        .collect();
    let mut sites = Sites::new(name, &read);
    for s in stmts {
        walk_stmt(ast, s, &mut frame(&rebound), &mut sites);
    }
    sites.clean
        && sites
            .found
            .iter()
            .all(|s| callees.contains(s) && !in_closures.found.contains(s))
}

/// A walk frame binding nothing, so every reference to `rebound` —
/// global spellings included — reports.
fn frame(rebound: &[String]) -> Bound<'_> {
    Bound {
        names: Vec::new(),
        rebound_globals: rebound,
    }
}

/// Every function literal the list's own statements hold, outermost
/// only (a walk of one covers what it nests).
fn for_each_arrow(ast: &Ast, s: &Stmt, f: &mut dyn FnMut(ExprId)) {
    for a in arrow_nodes(ast, s) {
        f(a);
    }
    for c in stmt_children_ref(s) {
        for_each_arrow(ast, c, f);
    }
}

/// The references to `name` the walk leaves free. `clean` drops when
/// one cannot carry a pad that reads `read` as the declaration does:
/// one inside a nested declaration (its frame binds `arguments`), one
/// under a frame rebinding a name in `read`, or a lifted capture.
struct Sites<'a> {
    name: &'a str,
    read: &'a [String],
    found: Vec<ExprId>,
    clean: bool,
}

impl<'a> Sites<'a> {
    fn new(name: &'a str, read: &'a [String]) -> Self {
        Sites {
            name,
            read,
            found: Vec::new(),
            clean: true,
        }
    }
}

impl FreeSink for Sites<'_> {
    fn free(&mut self, name: &str, site: Option<ExprId>, bound: &[String]) {
        if name != self.name {
            return;
        }
        match site {
            Some(s)
                if !bound.iter().any(|b| b == "arguments")
                    && !self.read.iter().any(|r| bound.contains(r)) =>
            {
                self.found.push(s)
            }
            _ => self.clean = false,
        }
    }
}
