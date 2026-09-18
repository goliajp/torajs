//! `#x in o` on a unit class's STATIC private element (RFC
//! 20260917-class-eval-unit K4-4).
//!
//! A static private element is an own entry of ONE evaluation's class
//! object, and every evaluation's class object carries the same key.
//! So the own-entry probe alone answers true for another evaluation's
//! class object, where §13.10.1 asks whether `o` has THIS evaluation's
//! element. The parser's `__torajs_priv_in_op("__priv_C__x", o)` grows
//! a third operand, the running static `this`, and the lowering also
//! asks `o === this`.
//!
//! Only sites inside the class's static bodies and static initializers
//! are covered: that is where `this` is the evaluation's class object.
//! A site in an instance body keeps the bare probe (registered).

use super::desugar_with::walk::{expr_children, stmt_children_ref, stmt_exprs};
use super::private_brand::PRIV_IN_OP;
use super::{Ast, Expr, ExprId, StaticInit, Stmt};
use std::collections::HashSet;

/// The `__torajs_priv_in_op(key, o)` calls under a subtree whose key is
/// one of `keys`, arrow bodies included.
struct Sites<'a> {
    keys: &'a HashSet<String>,
    found: Vec<ExprId>,
}

impl Sites<'_> {
    fn list(&mut self, ast: &Ast, stmts: &[Stmt]) {
        for s in stmts {
            for e in stmt_exprs(s) {
                self.expr(ast, e);
            }
            self.list_refs(ast, &stmt_children_ref(s));
        }
    }

    fn list_refs(&mut self, ast: &Ast, stmts: &[&Stmt]) {
        for s in stmts {
            self.list(ast, core::slice::from_ref(*s));
        }
    }

    fn expr(&mut self, ast: &Ast, eid: ExprId) {
        match ast.get_expr(eid) {
            Expr::Call { callee, args } => {
                if let [key, _] = args.as_slice()
                    && matches!(ast.get_expr(*callee), Expr::Ident(n) if n == PRIV_IN_OP)
                    && let Expr::String(key) = ast.get_expr(*key)
                    && self.keys.contains(key.to_string_lossy().as_ref())
                {
                    self.found.push(eid);
                }
            }
            Expr::ArrowFn { body, .. } => self.list(ast, body),
            _ => {}
        }
        for c in expr_children(ast, eid) {
            self.expr(ast, c);
        }
    }
}

/// Grow every covered site with the static `this` of its class.
pub(super) fn guard_sites(ast: &mut Ast) {
    let mut sites: Vec<(ExprId, String)> = Vec::new();
    for s in &ast.stmts {
        let Stmt::ClassDecl {
            name,
            static_methods,
            static_init,
            ..
        } = s
        else {
            continue;
        };
        if !ast.eval_unit_classes.contains(name) {
            continue;
        }
        let keys: HashSet<String> = static_methods
            .iter()
            .map(|m| m.name.lossy().into_owned())
            .chain(static_init.iter().filter_map(|si| match si {
                StaticInit::Field(f) => Some(f.name.lossy().into_owned()),
                StaticInit::Block(_) => None,
            }))
            .filter(|k| k.contains("__priv_"))
            .collect();
        if keys.is_empty() {
            continue;
        }
        let mut walk = Sites {
            keys: &keys,
            found: Vec::new(),
        };
        for m in static_methods {
            walk.list(ast, &m.body);
        }
        for si in static_init {
            match si {
                StaticInit::Field(f) => walk.expr(ast, f.init),
                StaticInit::Block(b) => walk.list(ast, b),
            }
        }
        sites.extend(walk.found.into_iter().map(|e| (e, name.clone())));
    }
    for (eid, cname) in sites {
        // A static `this` exactly as the parser records one: pass 2
        // turns it into the evaluation's `__unit`.
        let this = ast.add_expr(Expr::This);
        ast.static_this_sites.insert(this, cname);
        if let Expr::Call { args, .. } = &mut ast.exprs[eid.0 as usize] {
            args.push(this);
        }
    }
}
