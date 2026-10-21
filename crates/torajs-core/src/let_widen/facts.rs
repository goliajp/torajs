//! What the walker knows of the whole program before it walks: the
//! generator factories, the names rebound to a non-nullish value, the
//! lifted bodies and which of them are closures', and the module's
//! own bindings.

use std::cell::OnceCell;
use std::collections::{HashMap, HashSet};

use super::{Family, Walker, classify};
use crate::ast::{Ast, Expr, ExprId, Param, Stmt};
use crate::check::Type;

/// Top-level FnDecls by name, with their parameters and bodies.
pub(super) type Lifted<'a> = HashMap<&'a str, (&'a [Param], &'a [Stmt])>;

impl<'a> Walker<'a> {
    /// A walker over `ast` with the module's bindings registered. The
    /// facts that need a pass over the whole program are gathered the
    /// first time the walk asks for them: a walk over one specialization
    /// (`super::spec`) mostly asks for none of them, and should not cost
    /// a pass over every expression for each one.
    pub(super) fn new(ast: &'a Ast, types: Option<&'a HashMap<ExprId, Type>>) -> Self {
        let mut w = Walker {
            ast,
            types,
            gen_factories: OnceCell::new(),
            non_nullish_rebinds: OnceCell::new(),
            lifted: OnceCell::new(),
            closure_bodies: OnceCell::new(),
            module: HashMap::new(),
            entered: HashSet::new(),
            scopes: vec![HashMap::new()],
            out: HashSet::new(),
        };
        // a named function reads and writes the module's bindings
        // wherever it is declared, before or after them
        for s in &ast.stmts {
            if let Stmt::LetDecl {
                name,
                mutable,
                type_ann,
                init,
                ..
            } = s
            {
                w.register_let(name, *mutable, type_ann.as_deref(), *init);
            }
        }
        w.module = w.scopes[0].clone();
        w
    }

    pub(super) fn gen_factories(&self) -> &HashSet<String> {
        (self.gen_factories).get_or_init(|| collect_gen_factories(self.ast))
    }

    pub(super) fn non_nullish_rebinds(&self) -> &HashSet<String> {
        (self.non_nullish_rebinds).get_or_init(|| collect_non_nullish_rebinds(self.ast))
    }

    pub(super) fn lifted(&self) -> &Lifted<'a> {
        self.lifted.get_or_init(|| {
            (self.ast.stmts.iter())
                .filter_map(|s| match s {
                    Stmt::FnDecl {
                        name, params, body, ..
                    } => Some((name.as_str(), (params.as_slice(), body.as_slice()))),
                    _ => None,
                })
                .collect()
        })
    }

    pub(super) fn is_closure_body(&self, name: &str) -> bool {
        let bodies = self.closure_bodies.get_or_init(|| {
            (self.ast.exprs.iter())
                .filter_map(|e| match e {
                    Expr::Closure { fn_name, .. } => Some(fn_name.as_str()),
                    _ => None,
                })
                .collect()
        });
        bodies.contains(name)
    }
}

/// Names assigned, anywhere in the program, an rhs that is not the
/// `null` / `undefined` literal — the name-keyed half of the nullish
/// rule (module doc). Scope-blind on purpose.
fn collect_non_nullish_rebinds(ast: &Ast) -> HashSet<String> {
    let mut out = HashSet::new();
    for e in &ast.exprs {
        if let Expr::Assign { target, value } = e
            && let Expr::Ident(name) = ast.get_expr(*target)
            && !matches!(classify(ast, *value), Some(Family::Null | Family::Undef))
        {
            out.insert(name.clone());
        }
    }
    out
}

fn collect_gen_factories(ast: &Ast) -> HashSet<String> {
    fn walk(stmts: &[Stmt], out: &mut HashSet<String>) {
        for s in stmts {
            match s {
                Stmt::FnDecl {
                    name,
                    return_type: Some(rt),
                    body,
                    ..
                } => {
                    if rt.starts_with("__Gen_") {
                        out.insert(name.clone());
                    }
                    walk(body, out);
                }
                Stmt::FnDecl { body, .. } => walk(body, out),
                Stmt::Block(inner) | Stmt::Multi(inner) => walk(inner, out),
                _ => {}
            }
        }
    }
    let mut out = HashSet::new();
    walk(&ast.stmts, &mut out);
    out
}
