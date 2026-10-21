//! What the walker knows of the whole program before it walks: the
//! generator factories, the names rebound to a non-nullish value, the
//! lifted bodies and which of them are closures', and the module's
//! own bindings.

use std::collections::{HashMap, HashSet};

use super::{Family, Walker, classify};
use crate::ast::{Ast, Expr, ExprId};
use crate::check::Type;

impl<'a> Walker<'a> {
    pub(super) fn new(ast: &'a Ast, types: Option<&'a HashMap<ExprId, Type>>) -> Self {
        let mut w = Walker {
            ast,
            types,
            gen_factories: collect_gen_factories(ast),
            non_nullish_rebinds: collect_non_nullish_rebinds(ast),
            lifted: ast
                .stmts
                .iter()
                .filter_map(|s| match s {
                    crate::ast::Stmt::FnDecl {
                        name, params, body, ..
                    } => Some((name.as_str(), (params.as_slice(), body.as_slice()))),
                    _ => None,
                })
                .collect(),
            closure_bodies: (ast.exprs.iter())
                .filter_map(|e| match e {
                    Expr::Closure { fn_name, .. } => Some(fn_name.as_str()),
                    _ => None,
                })
                .collect(),
            module: HashMap::new(),
            entered: HashSet::new(),
            scopes: vec![HashMap::new()],
            out: HashSet::new(),
        };
        // a named function reads and writes the module's bindings
        // wherever it is declared, before or after them
        for s in &ast.stmts {
            if let crate::ast::Stmt::LetDecl {
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
    fn walk(stmts: &[crate::ast::Stmt], out: &mut HashSet<String>) {
        for s in stmts {
            match s {
                crate::ast::Stmt::FnDecl {
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
                crate::ast::Stmt::FnDecl { body, .. } => walk(body, out),
                crate::ast::Stmt::Block(inner) | crate::ast::Stmt::Multi(inner) => walk(inner, out),
                _ => {}
            }
        }
    }
    let mut out = HashSet::new();
    walk(&ast.stmts, &mut out);
    out
}
