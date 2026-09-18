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
//!
//! The same static bodies are where an INSTANCE element's brand check
//! cannot read the running evaluation off `__this` — there is no
//! instance — so [`running_this`] hands every check its carrier: the
//! static `this` here, `__this` elsewhere.

use super::desugar_with::walk::{expr_children, stmt_children_ref, stmt_exprs};
use super::private_brand::PRIV_IN_OP;
use super::{Ast, Expr, ExprId, StaticInit, Stmt};
use std::collections::{HashMap, HashSet};

/// Every expression under a subtree, arrow bodies included.
#[derive(Default)]
struct Sites {
    found: Vec<ExprId>,
}

impl Sites {
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
        self.found.push(eid);
        if let Expr::ArrowFn { body, .. } = ast.get_expr(eid) {
            self.list(ast, body);
        }
        for c in expr_children(ast, eid) {
            self.expr(ast, c);
        }
    }
}

/// Each expression inside a unit class's static bodies and static
/// initializers, with that class: where the running evaluation is the
/// static `this`, not an instance's hidden slot.
pub(super) fn static_body_exprs(ast: &Ast) -> HashMap<ExprId, String> {
    let mut out = HashMap::new();
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
        let mut walk = Sites::default();
        for m in static_methods {
            walk.list(ast, &m.body);
        }
        for si in static_init {
            match si {
                StaticInit::Field(f) => walk.expr(ast, f.init),
                StaticInit::Block(b) => walk.list(ast, b),
            }
        }
        out.extend(walk.found.into_iter().map(|e| (e, name.clone())));
    }
    out
}

/// The expression that carries the running evaluation at `site`: the
/// static `this` inside a static body (exactly as the parser records
/// one — pass 2 turns it into the evaluation's `__unit`), the instance
/// `__this` anywhere else.
pub(super) fn running_this(
    ast: &mut Ast,
    in_static: &HashMap<ExprId, String>,
    site: ExprId,
) -> ExprId {
    match in_static.get(&site) {
        Some(cname) => {
            let this = ast.add_expr(Expr::This);
            ast.static_this_sites.insert(this, cname.clone());
            this
        }
        None => ast.add_expr(Expr::Ident("__this".into())),
    }
}

/// The static private element keys a class declares.
fn static_private_keys(cls: &Stmt) -> HashSet<String> {
    let Stmt::ClassDecl {
        static_methods,
        static_init,
        ..
    } = cls
    else {
        return HashSet::new();
    };
    static_methods
        .iter()
        .map(|m| m.name.lossy().into_owned())
        .chain(static_init.iter().filter_map(|si| match si {
            StaticInit::Field(f) => Some(f.name.lossy().into_owned()),
            StaticInit::Block(_) => None,
        }))
        .filter(|k| k.contains("__priv_"))
        .collect()
}

/// Grow every covered site with the static `this` of its class.
pub(super) fn guard_sites(ast: &mut Ast, in_static: &HashMap<ExprId, String>) {
    let keys: HashMap<&str, HashSet<String>> = ast
        .stmts
        .iter()
        .filter_map(|s| match s {
            Stmt::ClassDecl { name, .. } => Some((name.as_str(), static_private_keys(s))),
            _ => None,
        })
        .collect();
    let mut sites: Vec<ExprId> = in_static
        .iter()
        .filter(|(eid, cname)| {
            let Expr::Call { callee, args } = ast.get_expr(**eid) else {
                return false;
            };
            let [key, _] = args.as_slice() else {
                return false;
            };
            matches!(ast.get_expr(*callee), Expr::Ident(n) if n == PRIV_IN_OP)
                && matches!(ast.get_expr(*key), Expr::String(k)
                    if keys.get(cname.as_str()).is_some_and(|ks| ks.contains(k.to_string_lossy().as_ref())))
        })
        .map(|(eid, _)| *eid)
        .collect();
    sites.sort_unstable_by_key(|e| e.0);
    for eid in sites {
        let this = running_this(ast, in_static, eid);
        if let Expr::Call { args, .. } = &mut ast.exprs[eid.0 as usize] {
            args.push(this);
        }
    }
}
